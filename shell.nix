{ norust ? false, devrustup ? true, rust-profile ? "stable" }:
let
  sources = import ./nix/sources.nix;
  pkgs = import sources.nixpkgs {
    overlays = [ (_: _: { inherit sources; }) (import ./nix/overlay.nix { }) (import sources.rust-overlay) ];
  };
in
with pkgs;
let
  norust_moth =
    "You have requested an environment without rust, you should provide it!";
  devrustup_moth =
    "You have requested an environment for rustup, you should provide it!";
  channel = import ./mayastor/nix/lib/rust.nix { inherit pkgs; };
  rust_chan = channel.default_src;
  rust = rust_chan.${rust-profile};
  k8sShellAttrs = import ./scripts/k8s/shell.nix { inherit pkgs; };
  helmShellAttrs = import ./charts/shell.nix { inherit pkgs; };
  stagingShellAttrs = import ./scripts/staging/shell.nix { inherit pkgs; };
  usePreCommit = builtins.getEnv "IN_NIX_SHELL" == "impure" && builtins.getEnv "CI" != "1";
  pre-commit = pkgs.runCommand "pre-commit" { } ''
    mkdir -p $out/bin
    cp ${pkgs.pre-commit}/bin/pre-commit $out/bin/pre-commit
  '';
in
mkShell {
  name = "openebs-shell";
  buildInputs = [
    cargo-expand
    cargo-udeps
    commitlint
    cowsay
    git
    nixpkgs-fmt
    paperclip
    openssl
    pkg-config
    which
    codespell
  ] ++ pkgs.lib.optional (!norust) rust
  ++ pkgs.lib.optional (usePreCommit) pre-commit
  ++ k8sShellAttrs.buildInputs ++ helmShellAttrs.buildInputs ++ stagingShellAttrs.buildInputs
  ++ pkgs.lib.optional (system == "aarch64-darwin") darwin.apple_sdk.frameworks.Security;

  PROTOC = "${protobuf}/bin/protoc";
  PROTOC_INCLUDE = "${protobuf}/include";

  # using the nix rust toolchain
  USE_NIX_RUST = "${toString (!norust)}";
  # copy the rust toolchain to a writable directory, see: https://github.com/rust-lang/cargo/issues/10096
  # the whole toolchain is copied to allow the src to be retrievable through "rustc --print sysroot"
  RUST_TOOLCHAIN = ".rust-toolchain/${rust.version}";
  RUST_TOOLCHAIN_NIX = "${rust}";

  shellHook = ''
    ./scripts/nix/git-submodule-init.sh
    if [ "${toString usePreCommit}" = "1" ]; then
      echo
      pre-commit install
      pre-commit install --hook commit-msg
    fi
    export OPENEBS_SRC=`pwd`
    export CTRL_SRC="$OPENEBS_SRC"/mayastor/dependencies/control-plane
    export PATH="$PATH:$(pwd)/target/debug"

    ${pkgs.lib.optionalString (norust) "cowsay ${norust_moth}"}
    ${pkgs.lib.optionalString (norust) "echo"}

    rust_version="${rust.version}" rustup_channel="${lib.strings.concatMapStringsSep "-" (x: x) (lib.lists.drop 1 (lib.strings.splitString "-" rust.version))}" \
    dev_rustup="${toString (devrustup)}" devrustup_moth="${devrustup_moth}" . "$CTRL_SRC"/scripts/rust/env-setup.sh
  '';
}
