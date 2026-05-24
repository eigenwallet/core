{
  description = "xmr-btc-swap / eigenwallet build environment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.05";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let pkgs = import nixpkgs { inherit system; };
      in {
        # nvidiaVersion is pinned to null here because flake evaluation is pure:
        # shell.nix's default auto-detection reads /proc and uses builtins.currentTime,
        # both of which a pure `nix develop` rejects. So the flake gives a working
        # shell with mesa software rendering. For GPU acceleration use `nix-shell`
        # (auto-detects the host driver), or `nix develop --impure` with an explicit
        # `nvidiaVersion`.
        devShells.default = import ./shell.nix {
          inherit pkgs;
          nvidiaVersion = null;
        };
      });
}
