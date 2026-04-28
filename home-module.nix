{ config, lib, pkgs, ... }:

let cfg = config.services.cider-mpris;
in {
  options.services.cider-mpris = {
    enable = lib.mkEnableOption "Cider MPRIS bridge";

    package = lib.mkOption {
      type = lib.types.package;
      description = "The cider-mpris package to use (set by the flake)";
    };
  };

  config = lib.mkIf cfg.enable {
    home.packages = [ cfg.package ];

    # The env file is created as a placeholder. You must edit it with your
    # Cider RPC token, or set CIDER_RPC_TOKEN in your environment another way.
    # Without the token, the service will fail to start.
    xdg.configFile."cider-mpris/env".text = ''
      # Add your CIDER_RPC_TOKEN here.
      # Find it in Cider: Settings → RPC Token
      CIDER_RPC_TOKEN=
    '';

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
