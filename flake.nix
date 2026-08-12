{
  description = "lean-ffi Nix flake (Lean4 + Rust)";

  nixConfig = {
    extra-substituters = [
      "https://argumentcomputer.cachix.org"
    ];
    extra-trusted-public-keys = [
      "argumentcomputer.cachix.org-1:ovhbTx1V56BYDerOWInQvXKXl68LlhNwEA+n7EWk1m4="
    ];
  };

  inputs = {
    # System packages, follows lean4-nix so we stay in sync
    nixpkgs.follows = "lean4-nix/nixpkgs";

    # Lean 4 & Lake
    lean4-nix.url = "github:argumentcomputer/lean4-nix";

    # Helper: flake-parts for easier outputs
    flake-parts.url = "github:hercules-ci/flake-parts";

    # Rust-related inputs
    fenix = {
      url = "github:nix-community/fenix";
      # Follow lean4-nix nixpkgs so we stay in sync
      inputs.nixpkgs.follows = "lean4-nix/nixpkgs";
    };

    crane.url = "github:ipetkov/crane";
  };

  outputs = inputs @ {
    flake-parts,
    lean4-nix,
    fenix,
    crane,
    ...
  }:
    flake-parts.lib.mkFlake {inherit inputs;} {
      # Systems we want to build for
      systems = [
        "aarch64-darwin"
        "aarch64-linux"
        "x86_64-darwin"
        "x86_64-linux"
      ];

      perSystem = {
        system,
        pkgs,
        ...
      }: let
        # Pins the Lean toolchain; a plain derivation, no overlay involved
        lean = lean4-nix.lib.${system}.fromToolchainFile ./lean-toolchain;

        # Pins the Rust toolchain
        rustToolchain = fenix.packages.${system}.fromToolchainFile {
          file = ./rust-toolchain.toml;
          sha256 = "sha256-sqSWJDUxc+zaz1nBWMAJKTAGBuGWP25GCftIOlCEAtA=";
        };

        # Rust package
        craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;
        src = craneLib.cleanCargoSource ./.;
        craneArgs = {
          inherit src;
          strictDeps = true;

          # build.rs uses LEAN_SYSROOT to locate lean/lean.h for bindgen
          LEAN_SYSROOT = "${lean}";
          # bindgen needs libclang to parse C headers
          LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";

          buildInputs =
            []
            ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
              # Additional darwin specific inputs can be set here
              pkgs.libiconv
            ];
        };
        # Build dependencies once and share them across the package build, the
        # test static lib, and the clippy check instead of recompiling per consumer.
        cargoArtifacts = craneLib.buildDepsOnly craneArgs;
        rustPkg = craneLib.buildPackage (
          craneArgs
          // {
            inherit cargoArtifacts;
            cargoExtraArgs = "--locked --workspace";
          }
        );
        # Static lib for the Lean FFI test suite; the Lean `lean-ffi-test` check
        # is where that suite runs, so skip the Rust checkPhase here.
        rustPkgTest = craneLib.buildPackage (
          craneArgs
          // {
            inherit cargoArtifacts;
            cargoExtraArgs = "--locked -p lean-ffi --features test-ffi";
            doCheck = false;
          }
        );

        # Lake test package
        lake2nix = pkgs.callPackage lean4-nix.lake {inherit lean;};
        lakeTest = lake2nix.mkPackage {
          name = "LeanFFITests";
          src = lake2nix.cleanLakeSource ./.;
          # Don't build the Rust static lib with Lake, since we build it with Crane
          postPatch = ''
            substituteInPlace lakefile.lean \
              --replace-fail 'proc { cmd := "cargo"' '--proc { cmd := "cargo"' \
              --replace-fail 'proc { cmd := "cp"' '--proc { cmd := "cp"'
          '';
          # Link the Rust static lib so Lake can find it
          postConfigure = ''
            mkdir -p target/release
            ln -s ${rustPkgTest}/lib/liblean_ffi.a target/release/liblean_ffi_test.a
          '';
        };
      in {
        packages = {
          default = rustPkg;
        };

        checks = {
          # Lint the Rust workspace; warnings are errors.
          clippy = craneLib.cargoClippy (
            craneArgs
            // {
              inherit cargoArtifacts;
              cargoClippyExtraArgs = "--workspace --all-targets --all-features -- -D warnings";
            }
          );
          # Build and run the Lean FFI test suite as a flake check.
          lean-ffi-test = pkgs.runCommand "lean-ffi-test" {} ''
            ${lakeTest}/bin/LeanFFITests
            touch $out
          '';
        };

        # Provide a unified dev shell with Lean + Rust
        devShells.default = pkgs.mkShell {
          # Disable fortify hardening as it causes warnings with cargo debug builds
          hardeningDisable = ["fortify"];
          # Add libclang for FFI with rust-bindgen
          LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
          packages = with pkgs; [
            clang
            rustToolchain
            rust-analyzer
            lean # Includes Lean compiler, lake, stdlib, etc.
            valgrind
          ];
        };

        formatter = pkgs.alejandra;
      };
    };
}
