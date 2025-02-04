# `eha` the /etc/hosts adder

This is a personal tool I use to quickly add `*.local`/`*.localhost` DNS entries to my /etc/hosts file to support local testing without needing to run a local BIND/DNSMASQ process. Each entry becomes eligible for cleanup after some expiry time (default 24h).

It has a very simple UX, to add a record:

```
eha add myapp.local
eha add myapp.local -e 60
```

Remove a record:

```
eha remove myapp.local
```

Note that both `add`, `remove`, and `remove-expired` will drop any items that are past their expiry time.

By default, this will read `/etc/hosts` and write to it afterwards, but, you can use `--file` to change the subject file, and `--test` to print the result to stdout without overwriting the file. 

## Install

```
cargo install eha
```

## Todo:

- [ ] publish crate
- [ ] licence
- [ ] write a blog post
