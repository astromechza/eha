use crate::Entry::{Other, Supported};
use anyhow::{anyhow, Context, Error};
use clap::Parser;
use jiff::{SignedDuration, Timestamp};
use rand::random;
use serde::{Deserialize, Serialize};
use std::env::current_dir;
use std::fs::{rename, File};
use std::io::{BufRead, BufReader, Write};
use std::ops::Add;

fn main() {
    if let Err(e) = main_err() {
        for ee in e.chain() {
            eprintln!("{}", ee);
        }
    }
}

fn main_err() -> Result<(), Error> {
    let args = Args::try_parse()?;
    args.validate()?;
    if let Some(contents) = args.run()? {
        println!("{}", contents);
    }
    Ok(())
}

#[derive(Parser, Debug)]
#[command(
    version,
    about = "eha (etc-hosts-adder) adds, removes, or expires temporary localhost names from the /etc/hosts file."
)]
struct Args {
    #[arg(help = "The DNS name ending in .local or .localhost to add or remove.")]
    name: Option<String>,

    #[arg(short, long, help = "Remove the given DNS name if present.")]
    remove: bool,

    #[arg(
        short,
        long,
        help = "Expiry in minutes for the entry, the entry is subject to removal after this time.",
        default_value = "1440"
    )]
    expire_minutes: usize,

    #[clap(long, help = "Operate on the given hosts file.", default_value = "/etc/hosts")]
    input_file: String,

    #[arg(long, help = "Print the new content to stdout instead of attempting to write the file.")]
    test: bool,
}

impl Args {
    fn validate(&self) -> Result<(), Error> {
        if let Some(name) = &self.name {
            if !name.ends_with(".local") && !name.ends_with(".localhost") {
                return Err(anyhow!("name must end in .local or .localhost"));
            } else if !(1..525600).contains(&self.expire_minutes) {
                return Err(anyhow!("ttl minutes must be between 1m and 365d (inclusive)"));
            } else {
                for (i, x) in name.split('.').enumerate() {
                    let l = x.len();
                    if l == 0 {
                        return Err(anyhow!("invalid DNS name #{}: cannot be empty", i));
                    } else if let Some((j, c, _)) = x.chars().enumerate().map(|(a, b)| (a, b, l)).find(invalid_dns_name_char) {
                        return Err(anyhow!("invalid DNS name char in part #{} @ {}: {}", i, j, c));
                    }
                }
            }
        }
        Ok(())
    }

    fn run(&self) -> Result<Option<String>, Error> {
        let mut entries: Vec<Entry> = Vec::new();
        {
            let file = File::open(&self.input_file).context("failed to read input file")?;
            let reader = BufReader::new(file);
            for line in reader.lines() {
                let line = line.context("failed to read line")?;
                entries.push(Entry::from(line.as_str()));
            }
        }
        eprintln!("read {} entries from existing file {}", entries.len(), &self.input_file);

        let now = Timestamp::now();
        let mut after_entries = entries
            .iter()
            .filter(|e| match (self.name.as_ref(), e) {
                // filter out expired items
                (_, Supported { meta, .. }) if meta.expiry < now => false,
                // filter out items that have the target name
                (Some(n), Supported { name, .. }) if name == n => false,
                // keep anything else
                (_, _) => true,
            })
            .map(String::from)
            .collect::<Vec<_>>();

        if let Some(n) = self.name.as_ref() {
            if !self.remove {
                after_entries.push(String::from(&Supported {
                    name: n.to_string(),
                    meta: SupportedMeta {
                        expiry: now.add(SignedDuration::from_mins(self.expire_minutes as i64)),
                        comment: Some(format!("set from {} at {}", current_dir().unwrap_or_default().to_string_lossy(), &now,).to_string()),
                    },
                }))
            }
        }

        if self.test {
            return Ok(Some(after_entries.join("\n")));
        }

        let mut temp_file_path = std::env::temp_dir();
        temp_file_path.push(format!("hosts{}", random::<u32>()));
        eprintln!(
            "writing to {} and moving to {}",
            &temp_file_path.to_string_lossy(),
            &self.input_file
        );
        let mut file = File::create(&temp_file_path).context("failed to create temp file")?;
        file.write_all(after_entries.join("\n").as_bytes())
            .context("failed to write content")?;
        rename(&temp_file_path, &self.input_file).context("failed to rename temp file to input file")?;
        Ok(None)
    }
}

/// Returns whether the given character is invalid in a DNS name. This designed to be used as a
/// chained filter.
fn invalid_dns_name_char(bits: &(usize, char, usize)) -> bool {
    let (index, c, part_len) = *bits;

    // cannot be longer than 63
    if part_len > 63 {
        return true;
    }
    // cannot start or end with -
    if c == '-' && (index == 0 || index == part_len - 1) {
        return true;
    }
    // must be valid char
    !c.is_ascii_alphanumeric() && c != '-'
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct SupportedMeta {
    expiry: Timestamp,
    comment: Option<String>,
}

enum Entry {
    Supported { name: String, meta: SupportedMeta },
    Other(String),
}

impl From<&str> for Entry {
    fn from(value: &str) -> Self {
        if let Some((a, b)) = value.split_once("# eha ") {
            if let Some(name) = a.split_whitespace().last() {
                return Supported {
                    name: name.to_string(),
                    meta: serde_json::from_str(b).unwrap_or_default(),
                };
            }
        }
        Other(value.to_string())
    }
}

impl From<&Entry> for String {
    fn from(value: &Entry) -> Self {
        match value {
            Supported { name, meta } => format!(
                "127.0.0.1\t{}\t# eha {}",
                name,
                serde_json::to_string(meta).unwrap_or_else(|e| e.to_string())
            ),
            Other(raw) => raw.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use tempfile::NamedTempFile;

    #[test]
    fn test_no_op() -> Result<(), Error> {
        let mut f = NamedTempFile::new()?;
        let input = r##"# some leading comments followed by whitespace

127.0.0.1   localhost
10.0.0.9    other.name
127.0.0.1	foo.local	# eha {"expiry":"2030-01-01T00:00:00Z","comment":"hello world"}"##;
        f.write_all(input.as_bytes())?;
        let args = Args {
            name: None,
            remove: false,
            expire_minutes: 1,
            input_file: f.path().to_string_lossy().to_string(),
            test: true,
        };
        args.validate()?;
        let content = args.run()?.unwrap_or_default();
        println!("{}", content);
        assert_eq!(content, input);
        Ok(())
    }

    #[test]
    fn test_remove_expired_while_adding() -> Result<(), Error> {
        let mut f = NamedTempFile::new()?;
        f.write_all(
            br##"# some leading comments followed by whitespace

127.0.0.1   localhost
10.0.0.9    other.name
127.0.0.1	foo.local	# eha {"expiry":"2001-01-01T00:00:00Z","comment":"hello world"}"##,
        )?;
        let args = Args {
            name: Some("thing.local".to_string()),
            remove: false,
            expire_minutes: 1,
            input_file: f.path().to_string_lossy().to_string(),
            test: true,
        };
        args.validate()?;
        let content = args.run()?.unwrap_or_default();
        println!("{}", content);
        assert!(content.contains("127.0.0.1\tthing.local\t# eha {"));
        assert!(!content.contains("127.0.0.1\tfoo.local\t# eha {"));
        Ok(())
    }

    #[test]
    fn test_remove_entry() -> Result<(), Error> {
        let mut f = NamedTempFile::new()?;
        f.write_all(
            br##"# some leading comments followed by whitespace

127.0.0.1   localhost
10.0.0.9    other.name
127.0.0.1	foo.local	# eha {"expiry":"2030-01-01T00:00:00Z","comment":"hello world"}"##,
        )?;
        let args = Args {
            name: Some("foo.local".to_string()),
            remove: true,
            expire_minutes: 1,
            input_file: f.path().to_string_lossy().to_string(),
            test: true,
        };
        args.validate()?;
        let content = args.run()?.unwrap_or_default();
        println!("{}", content);
        assert_eq!(
            content,
            r##"# some leading comments followed by whitespace

127.0.0.1   localhost
10.0.0.9    other.name"##
        );
        Ok(())
    }

    #[test]
    fn test_overwrite_file() -> Result<(), Error> {
        let mut f = NamedTempFile::new()?;
        f.write_all(
            br##"# some leading comments followed by whitespace

127.0.0.1   localhost
10.0.0.9    other.name"##,
        )?;
        let args = Args {
            name: Some("foo.local".to_string()),
            remove: false,
            expire_minutes: 1,
            input_file: f.path().to_string_lossy().to_string(),
            test: false,
        };
        args.validate()?;
        assert!(args.run()?.is_none());

        let mut f2 = File::open(f.path())?;
        let mut content = String::new();
        f2.read_to_string(&mut content)?;
        assert!(content.contains("127.0.0.1\tfoo.local\t# eha {"));

        Ok(())
    }
}
