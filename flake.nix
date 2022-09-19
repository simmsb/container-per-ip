{
  description = "container per ip";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

    crane = {
      url = "github:ipetkov/crane";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    flake-utils.url = "github:numtide/flake-utils";

    advisory-db = {
      url = "github:rustsec/advisory-db";
      flake = false;
    };

    nix2container.url = "github:nlewo/nix2container";
  };

  outputs = { self, nixpkgs, crane, flake-utils, advisory-db, nix2container, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
        };

        inherit (pkgs) lib;

        craneLib = crane.lib.${system};
        src = ./.;

        # Build *just* the cargo dependencies, so we can reuse
        # all of that work (e.g. via cachix) when running in CI
        cargoArtifacts = craneLib.buildDepsOnly {
          inherit src;
        };

        # Build the actual crate itself, reusing the dependency
        # artifacts from above.
        my-crate = craneLib.buildPackage {
          inherit cargoArtifacts src;
        };
      in
      {
        checks = {
          # Build the crate as part of `nix flake check` for convenience
          inherit my-crate;

          my-crate-clippy = craneLib.cargoClippy {
            inherit cargoArtifacts src;
            cargoClippyExtraArgs = "--all-targets -- --deny warnings";
          };

          # Audit dependencies
          my-crate-audit = craneLib.cargoAudit {
            inherit src advisory-db;
          };
        };

        packages.default = my-crate;

        packages.oci-image = nix2container.packages.${system}.nix2container.buildImage {
          name = "simmsb/container-per-ip";
          tag = "latest";
          config = {
            entrypoint = [ "${my-crate}/bin/container-per-ip" ];
          };
        };

        apps.default = flake-utils.lib.mkApp {
          drv = my-crate;
        };

        devShells.default = pkgs.mkShell {
          inputsFrom = builtins.attrValues self.checks;

          # Extra inputs can be added here
          nativeBuildInputs = with pkgs; [
            cargo
            rustc
          ];
        };
      });
}
