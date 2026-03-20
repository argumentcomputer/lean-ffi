{
  description = "lean-ffi Nix flake (Lean4 + Rust)";

  inputs = {
    # System packages, follows lean4-nix so we stay in sync
    nixpkgs.follows = "lean4-nix/nixpkgs";

    # Lean 4 & Lake
    lean4-nix.url = "github:lenianiva/lean4-nix";

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
    nixpkgs,
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
          LEAN_SYSROOT = "${pkgs.lean.lean-all}";
          # bindgen needs libclang to parse C headers
          LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";

          buildInputs =
            []
            ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
              # Additional darwin specific inputs can be set here
              pkgs.libiconv
            ];
        };
        rustPkg = craneLib.buildPackage (craneArgs // {cargoExtraArgs = "--locked --features test-ffi";});

        # Lake test package
        lake2nix = pkgs.callPackage lean4-nix.lake {};
        lakeTest = lake2nix.mkPackage {
          name = "LeanFFITests";
          src = ./.;
          # Don't build the Rust static lib with Lake, since we build it with Crane
          postPatch = ''
            substituteInPlace lakefile.lean \
              --replace-fail 'proc { cmd := "cargo"' '--proc { cmd := "cargo"' \
              --replace-fail 'proc { cmd := "cp"' '--proc { cmd := "cp"'
          '';
          # Link the Rust static lib so Lake can find it
          postConfigure = ''
            mkdir -p target/release
            ln -s ${rustPkg}/lib/liblean_ffi.a target/release/liblean_ffi_test.a
          '';
        };
      in {
        # Lean overlay
        _module.args.pkgs = import nixpkgs {
          inherit system;
          overlays = [(lean4-nix.readToolchainFile ./lean-toolchain)];
        };

        packages = {
          default = rustPkg;
          test = lakeTest;
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
            lean.lean-all # Includes Lean compiler, lake, stdlib, etc.
          ];
        };

        formatter = pkgs.alejandra;
      };
    };
}
