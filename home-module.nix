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

    # Only create the env file if it doesn't already exist (so home-manager
    # doesn't clobber the user's manual edits).
    home.activation.createCiderMprisEnv = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
      ENV_FILE="$HOME/.config/cider-mpris/env"
      if [ ! -f "$ENV_FILE" ]; then
        mkdir -p "$(dirname "$ENV_FILE")"
        cat > "$ENV_FILE" << 'EOF'
# Add your CIDER_RPC_TOKEN here.
# Find it in Cider: Settings → RPC Token
CIDER_RPC_TOKEN=
EOF
      fi
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
