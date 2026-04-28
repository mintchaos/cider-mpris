{ config, lib, pkgs, ... }:

let cfg = config.services.cider-mpris;
in {
  options.services.cider-mpris = {
    enable = lib.mkEnableOption "Cider MPRIS bridge";

    package = lib.mkOption {
      type = lib.types.package;
      description = "The cider-mpris package to use (set by the flake)";
    };

    rpcTokenFile = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = null;
      description = "Path to file containing CIDER_RPC_TOKEN";
    };
  };

  config = lib.mkIf cfg.enable {
    home.packages = [ cfg.package ];

    xdg.configFile."cider-mpris/env".text =
      if cfg.rpcTokenFile != null
      then "CIDER_RPC_TOKEN=${builtins.readFile cfg.rpcTokenFile}"
      else "# Add your CIDER_RPC_TOKEN here\nCIDER_RPC_TOKEN=";

    systemd.user.services.cider-mpris = {
      Unit = {
        Description = "Cider MPRIS Bridge";
        After = "graphical-session.target";
        Wants = "graphical-session.target";
      };
      Service = {
        Type = "simple";
        ExecStart = "${cfg.package}/bin/cider-mpris";
        EnvironmentFile = "${config.xdg.configHome}/cider-mpris/env";
        Restart = "on-failure";
        RestartSec = 5;
      };
      Install.WantedBy = [ "default.target" ];
    };
  };
}
