{
  description = "xmr-btc-swap / eigenwallet build environment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.05";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
        pkgsAndroid = import nixpkgs {
          inherit system;
          config = {
            allowUnfree = true;
            android_sdk.accept_license = true;
          };
        };
      in {
        devShells.default = import ./shell.nix {
          inherit pkgs;
          nvidiaVersion = null;
        };
        devShells.android = import ./shell.nix {
          pkgs = pkgsAndroid;
          nvidiaVersion = null;
          withAndroid = true;
        };
      });
}
