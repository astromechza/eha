use crate::Entry::{Other, Supported};
use anyhow::{anyhow, Context, Error};
use clap::Parser;
use jiff::{SignedDuration, Timestamp};
use rand::random;
use std::env::current_dir;
use std::fs::{rename, File};
use std::io::{BufRead, BufReader, Write};
use std::ops::Add;

#[derive(Parser, Debug)]
#[command(version)]
struct Args {
    name: Option<String>,

    #[arg(short, long, help = "Remove the given name if present")]
    remove: bool,

    #[arg(
        short,
        long,
        help = "Expiry in minutes for the entry",
        default_value = "1440"
    )]
    expire_minutes: usize,

    #[clap(
        long,
        help = "Operate on the given hosts file",
        default_value = "/etc/hosts"
    )]
    input_file: String,

    #[arg(long, help = "Print the new content to stdout instead of the file")]
    test: bool,
}

fn main() {
    if let Err(e) = main_err() {
        for ee in e.chain() {
            eprintln!("{}", ee);
        }
    }
}

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

fn main_err() -> Result<(), Error> {
    let args = Args::try_parse()?;

    // first apply validation of arguments
    if let Some(name) = &args.name {
        if !name.ends_with(".local") && !name.ends_with(".localhost") {
            return Err(anyhow!("name must end in .local or .localhost"));
        } else if !(1..525600).contains(&args.expire_minutes) {
            return Err(anyhow!(
                "ttl minutes must be between 1m and 365d (inclusive)"
            ));
        } else {
            for (i, x) in name.split('.').enumerate() {
                let l = x.len();
                if l == 0 {
                    return Err(anyhow!("invalid DNS name #{}: cannot be empty", i));
                } else if let Some((j, c, _)) = x
                    .chars()
                    .enumerate()
                    .map(|(a, b)| (a, b, l))
                    .find(invalid_dns_name_char)
                {
                    return Err(anyhow!(
                        "invalid DNS name char in part #{} @ {}: {}",
                        i,
                        j,
                        c
                    ));
                }
            }
        }
    }

    let mut entries: Vec<Entry> = Vec::new();
    {
        let file = File::open(&args.input_file).context("failed to read input file")?;
        let reader = BufReader::new(file);
        for line in reader.lines() {
            let line = line.context("failed to read line")?;
            entries.push(Entry::from(line.as_str()));
        }
    }
    eprintln!(
        "read {} entries from existing file {}",
        entries.len(),
        &args.input_file
    );

    let now = Timestamp::now();
    let new_item = args
        .name
        .as_ref()
        .filter(|_| !&args.remove)
        .inspect(|n| eprintln!("appending new entry for {}", n))
        .map(|n| Supported {
            name: n.to_string(),
            expiry: now.add(SignedDuration::from_mins(args.expire_minutes as i64)),
            comment: Some(
                format!(
                    "set from {} at {}",
                    current_dir().unwrap_or_default().to_string_lossy(),
                    &now,
                )
                .to_string(),
            ),
        });

    let after = entries
        .iter()
        .filter(|e| {
            if let Some(n) = args.name.as_ref() {
                if let Supported { name, expiry, .. } = e {
                    if expiry > &now && name.ne(n) {
                        eprintln!("skipping existing item for {}", name);
                        return false;
                    }
                }
            }
            true
        })
        .chain(new_item.iter())
        .map(String::from)
        .collect::<Vec<_>>()
        .join("\n");

    if args.test {
        println!("{}", after);
    } else {
        let mut temp_file_path = std::env::temp_dir();
        temp_file_path.push(format!("hosts{}", random::<u32>()));
        {
            eprintln!(
                "writing to {} and moving to {}",
                &temp_file_path.to_string_lossy(),
                &args.input_file
            );
            let mut file = File::create(&temp_file_path).context("failed to create temp file")?;
            file.write_all(after.as_bytes())
                .context("failed to write content")?;
        }
        rename(&temp_file_path, &args.input_file)
            .context("failed to rename temp file to input file")?;
    }

    Ok(())
}

enum Entry {
    Supported {
        name: String,
        expiry: Timestamp,
        comment: Option<String>,
    },
    Other(String),
}

impl From<&str> for Entry {
    fn from(value: &str) -> Self {
        if let Some((a, b)) = value.split_once("# eha ") {
            if let Some(name) = a.split_whitespace().last() {
                let mut exp = Timestamp::default();
                let mut comment: Option<String> = None;
                for x in b.split(',') {
                    if let Some((k, v)) = x.split_once('=') {
                        match k {
                            "expiry" => exp = v.parse().unwrap_or_default(),
                            "comment" => comment = Some(v.to_string()),
                            _ => {}
                        }
                    }
                }
                return Supported {
                    name: name.to_string(),
                    expiry: exp,
                    comment,
                };
            }
        }
        Other(value.to_string())
    }
}

impl From<&Entry> for String {
    fn from(value: &Entry) -> Self {
        match value {
            Supported {
                name,
                expiry,
                comment,
            } => format!(
                "127.0.0.0\t{}\t# eha exp={},comment={}\n",
                name,
                expiry,
                comment.as_ref().map(|s| s.as_str()).unwrap_or("")
            ),
            Other(raw) => raw.to_string(),
        }
    }
}
