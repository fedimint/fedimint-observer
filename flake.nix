{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    flakebox = {
      url = "github:rustshop/flakebox";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-utils, flakebox }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
        };
        flakeboxLib = flakebox.lib.mkLib pkgs { };
        lib = pkgs.lib;

        toolchains = flakeboxLib.mkFenixToolchain {
          components = [
            "rustc"
            "cargo"
            "clippy"
            "rust-analyzer"
            "rust-src"
          ];

          args = {
            nativeBuildInputs = with pkgs; [
              # cmake is required for building aws-lc-sys (fedimint dependency)
              cmake
            ];
          };
          targets = (pkgs.lib.getAttrs
            ([
              "default"
              "wasm32-unknown"
            ])
            (flakeboxLib.mkStdTargets { })
          );
        };

        rustSrc = flakeboxLib.filterSubPaths {
          root = builtins.path {
            name = "fmo";
            path = ./.;
          };
          paths = [
            "Cargo.toml"
            "Cargo.lock"
            ".cargo"
            "fmo_api_types"
            "fmo_server"
          ];
        };

        nativePackages =
          (flakeboxLib.craneMultiBuild { toolchains = { default = toolchains; }; }) (craneLib':
            let
              craneLib = (craneLib'.overrideArgs {
                pname = "fmo_server";
                version = "0.1.0";
                src = rustSrc;
                cargoExtraArgs = "--package=fmo_server";
                RUSTFLAGS = "--cfg tokio_unstable";
              });
            in
            rec {
              workspaceDeps = craneLib.buildWorkspaceDepsOnly { };
              workspaceBuild = craneLib.buildWorkspace {
                cargoArtifacts = workspaceDeps;
              };
              fmo_server = craneLib.buildPackage { };
              fmo_server_image = pkgs.dockerTools.buildLayeredImage {
                name = "fmo_server";
                contents = [ fmo_server pkgs.bash pkgs.coreutils ];
                config = {
                  Cmd = [
                    "${fmo_server}/bin/fmo_server"
                  ];
                };
              };

            });

        reactPackages =
          let
            # Get the npm dependencies hash
            # To update: nix build .#fmo_frontend_react_default --impure
            # and use the hash from the error message
            npmDepsHash = "sha256-d04Zjrg1mOhnO7FgG6rvDSh0ovt+z7PqIE8FCSG2Czk=";
          in
          rec {
            fmo_frontend_react = api: pkgs.buildNpmPackage {
              pname = "fmo_frontend_react";
              version = "0.1.0";

              src = pkgs.lib.cleanSourceWith {
                src = ./fmo_frontend_react;
                filter = path: type:
                  let
                    baseName = baseNameOf path;
                  in
                  # Exclude common unnecessary files
                  baseName != "node_modules" &&
                  baseName != "dist" &&
                  baseName != ".git";
              };

              inherit npmDepsHash;

              # Set the API base URL environment variable
              VITE_FMO_API_BASE_URL = api;

              # Build command as defined in package.json
              buildPhase = ''
                runHook preBuild
                npm run build
                runHook postBuild
              '';

              # Install the built artifacts
              installPhase = ''
                runHook preInstall
                mkdir -p $out
                cp -r dist/* $out/
                # Copy index.html to 404.html for client-side routing (as done in CI)
                cp $out/index.html $out/404.html
                runHook postInstall
              '';

              meta = with pkgs.lib; {
                description = "Fedimint Observer React Frontend";
                license = licenses.mit;
              };
            };

            fmo_frontend_react_default = fmo_frontend_react "http://localhost:3000/api";
          };
      in
      {
        devShells = flakeboxLib.mkShells {
          toolchain = toolchains;

          nativeBuildInputs = [
            pkgs.postgresql
            # sqlite is used only for creating the dump file for migrating existing instances
            pkgs.sqlite
            pkgs.nixpkgs-fmt
            # cmake is required for building aws-lc-sys (fedimint dependency)
            pkgs.cmake
          ];

          shellHook = ''
            source scripts/pg_dev/env.sh
            echo "Type 'just pg_start' to start the $PGDATABASE database, use 'pg' to connect to it"
          '';

          RUSTFLAGS = "--cfg=web_sys_unstable_apis --cfg=tokio_unstable";
        };

        legacyPackages = nativePackages // reactPackages;
        packages.default = nativePackages.fmo_server;
      }
    );
}
