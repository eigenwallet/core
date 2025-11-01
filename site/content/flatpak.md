### Flatpak

For Linux systems with Flatpak support, you can install _eigenwallet_ directly from our Flatpak repository. Using the Flatpak may help if you have glibc-related issues running the binary release.

**Requirements:**
- [Flatpak 1.0 or newer](https://flatpak.org/setup/)
- x86_64 architecture

**Install**:
```sh
flatpak remote-add --if-not-exists eigenwallet https://eigenwallet.github.io/core/eigenwallet.flatpakrepo
flatpak install flathub org.gnome.Platform//47
flatpak install eigenwallet org.eigenwallet.app
```
**Run**:
```sh
flatpak run org.eigenwallet.app
```

**Update**:
```sh
flatpak update org.eigenwallet.app
```
