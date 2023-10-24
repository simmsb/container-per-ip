{
  description = "container per ip";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

    crane = {
      url = "github:ipetkov/crane";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    parts.url = "github:hercules-ci/flake-parts";
    parts.inputs.nixpkgs-lib.follows = "nixpkgs";

    nix2container.url = "github:nlewo/nix2container";
  };

  outputs =
    inputs@{ self
    , nixpkgs
    , crane
    , parts
    , nix2container
    , ...
    }:
    parts.lib.mkFlake { inherit inputs; } {
      systems = nixpkgs.lib.systems.flakeExposed;
      imports = [
      ];
      perSystem = { config, pkgs, system, lib, ... }:
        let
          craneLib = crane.lib.${system};
          src = craneLib.cleanCargoSource (craneLib.path ./.);

          commonArgs = {
            inherit src;
            strictDeps = true;

            buildInputs = [
              # Add additional build inputs here
            ] ++ lib.optionals pkgs.stdenv.isDarwin [
              # Additional darwin specific inputs can be set here
              pkgs.libiconv
            ];
          };

          cargoArtifacts = craneLib.buildDepsOnly commonArgs;
          my-crate = craneLib.buildPackage (commonArgs // {
            inherit cargoArtifacts;
          });

          checks = {
            # Build the crate as part of `nix flake check` for convenience
            inherit my-crate;

            # Run clippy (and deny all warnings) on the crate source,
            # again, resuing the dependency artifacts from above.
            #
            # Note that this is done as a separate derivation so that
            # we can block the CI if there are issues here, but not
            # prevent downstream consumers from building our crate by itself.
            my-crate-clippy = craneLib.cargoClippy (commonArgs // {
              inherit cargoArtifacts;
              cargoClippyExtraArgs = "--all-targets -- --deny warnings";
            });

            my-crate-doc = craneLib.cargoDoc (commonArgs // {
              inherit cargoArtifacts;
            });

            # Check formatting
            my-crate-fmt = craneLib.cargoFmt {
              inherit src;
            };

          };
        in
        {
          packages.default = my-crate;

          packages.oci-image = nix2container.packages.${system}.nix2container.buildImage {
            name = "simmsb/container-per-ip";
            tag = "latest";
            config = {
              entrypoint = [ "${my-crate}/bin/container-per-ip" ];
            };
          };


          devShells.default = craneLib.devShell {
            checks = self.checks.${system};

            RUST_SRC_PATH = pkgs.rustPlatform.rustLibSrc;

            packages = [
              pkgs.rust-analyzer
            ] ++ lib.optionals pkgs.stdenv.isDarwin [ pkgs.libiconv ];
          };
        };
    };
}
