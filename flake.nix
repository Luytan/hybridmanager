{
  description = "Chainsaw - dGPU manager for laptop";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    {
      self,
      nixpkgs,
      rust-overlay,
      flake-utils,
    }:
    let
      pkgsFor =
        system:
        import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
        };

      mkPackage =
        pkgs: release:
        let
          rustToolchain = pkgs.rust-bin.stable.latest.default;
          rustPlatform = pkgs.makeRustPlatform {
            cargo = rustToolchain;
            rustc = rustToolchain;
          };
        in
        rustPlatform.buildRustPackage {
          pname = "Chainsaw";
          version = "0.20.0";
          src = ./.;
          inherit release;
          cargoLock.lockFile = ./Cargo.lock;

          # hwdata is required for accessing hardware databases
          nativeBuildInputs = [ 
            pkgs.pkg-config 
            pkgs.clang
            pkgs.llvm
            pkgs.bpf-linker
            pkgs.bpftools
          ];
          buildInputs = [ 
            pkgs.hwdata 
            pkgs.libbpf
          ];

          # Patch the source code to point to the correct hwdata location in the Nix store
          postPatch = ''
            substituteInPlace crates/chainsaw-core/src/iommu.rs \
              --replace "/usr/share/hwdata/pci.ids" "${pkgs.hwdata}/share/hwdata/pci.ids"
          '';

          # Make D-Bus configuration available
          postInstall = ''
            mkdir -p $out/share/dbus-1/system.d
            install -m 644 ${./assets/com.chainsaw.daemon.conf} \
              $out/share/dbus-1/system.d/com.chainsaw.daemon.conf
          '';

          meta.mainProgram = "chainsaw";
        };

      nixosModule =
        {
          config,
          lib,
          pkgs,
          ...
        }:
        with lib;
        let
          cfg = config.services.chainsaw;
          package = mkPackage (pkgsFor pkgs.system) true;
        in
        {
          options.services.chainsaw = {
            enable = mkEnableOption "Chainsaw daemon";
            package = mkOption {
              type = types.package;
              default = package;
              description = "Chainsaw daemon package";
            };
          };

          config = mkIf cfg.enable {
            environment.systemPackages = [ cfg.package ];
            services.dbus.packages = [ cfg.package ];
            services.dbus.enable = true;

            systemd.services.chainsawd = {
              description = "Chainsaw Daemon";
              after = [
                "dbus.service"
                "network.target"
              ];
              requires = [ "dbus.service" ];

              before = [
                "graphical.target"
                "multi-user.target"
                "display-manager.service"
                "nvidia-powerd.service"
              ];

              serviceConfig = {
                Type = "dbus";
                BusName = "com.chainsaw.daemon";
                ExecStart = "${cfg.package}/bin/chainsawd";
                Restart = "on-failure";
                RestartSec = "5s";
                User = "root";

                Environment = [
                  "PATH=${
                    lib.makeBinPath [
                      pkgs.hwdata
                      pkgs.pciutils
                      pkgs.usbutils
                    ]
                  }:/run/current-system/sw/bin"
                ];
              };

              wantedBy = [ "multi-user.target" ];
            };
          };
        };
    in
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = pkgsFor system;
      in
      {
        packages.default = mkPackage pkgs true;
        packages.debug = mkPackage pkgs false;

        devShells.default = pkgs.mkShell {
          buildInputs = [
            pkgs.rust-bin.stable.latest.default
            pkgs.pkg-config
            pkgs.hwdata
            pkgs.bpf-linker
            pkgs.bpftools
            pkgs.clang
            pkgs.llvm
            pkgs.libbpf
          ];
        };
      }
    )
    // {
      nixosModules.default = nixosModule;
      nixosModules.chainsaw = nixosModule;
    };
}
