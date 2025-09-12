# eigenwallet

This is the monorepo containing the source code for all of our core projects:

- [`swap`](swap/README.md) contains the source code for the main swapping binaries, `asb` and `swap`
  - [`maker`](dev-docs/asb/README.md)
  - [`taker`](dev-docs/cli/README.md)
- [`gui`](src-gui/README.md) contains the new tauri based user interface
- [`tauri`](src-tauri/) contains the tauri bindings between binaries and user interface
- and other crates we use in our binaries

If you're just here for the software, head over to the [releases](https://github.com/eigenwallet/core/releases/latest) tab and grab the binary for your operating system! If you're just looking for documentation, check out our [docs page](https://docs.unstoppableswap.net/) or our [github docs](dev-docs/README.md).

Join our [Matrix room](https://matrix.to/#/#unstoppableswap-core:matrix.org) to follow development more closely.

> The project was previously known as UnstoppableSwap. Read [this](https://eigenwallet.org/rename.html) for our motivation for the rename.

<img width="1824" height="1624" alt="image" src="https://github.com/user-attachments/assets/d3838b57-95ea-486b-9db7-aecb88f1174a" />
<img width="1824" height="1624" alt="image" src="https://github.com/user-attachments/assets/4515198f-296a-4ea1-85be-ed23201056b7" />
<img width="2060" height="1578" alt="image" src="https://github.com/user-attachments/assets/5f043d23-bd31-4ec8-a21c-85744da5c0c3" />

## Contributing

We have a `justfile` containing a lot of useful commands.
Run `just help` to see all the available commands.

## Running tests

This repository uses [cargo-nextest](https://nexte.st/docs/running/) to run the
test suite.

```bash
cargo install cargo-nextest
cargo nextest run
```

## Donations

If you want to donate to the project, you can use the following address. Donations will be used to fund development.

Please only do so if you do not need the money. We'd rather you keep it but people ask from time to time so we're adding it here.

```gpg
-----BEGIN PGP SIGNED MESSAGE-----
Hash: SHA512

87QwQmWZQwS6RvuprCqWuJgmystL8Dw6BCx8SrrCjVJhZYGc5s6kf9A2awfFfStvEGCGeNTBNqLGrHzH6d4gi7jLM2aoq9o is our donation address for Github (signed by binarybaron)
-----BEGIN PGP SIGNATURE-----

iHUEARYKAB0WIQQ1qETX9LVbxE4YD/GZt10+FHaibgUCaJTWlQAKCRCZt10+FHai
bhasAQDGrAkZu+FFwDZDUEZzrIVS42he+GeMiS+ykpXyL5I7RQD/dXCR3f39zFsK
1A7y45B3a8ZJYTzC7bbppg6cEnCoWQE=
=j+Vz
-----END PGP SIGNATURE-----
```
