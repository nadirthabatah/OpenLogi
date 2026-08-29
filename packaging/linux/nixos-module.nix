{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.programs.roadie;
in
{
  options.programs.roadie = {
    enable = lib.mkEnableOption "OpenRoadie, a local-first Logitech device manager";

    package = lib.mkPackageOption pkgs "roadie" { };

    launchAtLogin = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Whether to start the OpenRoadie agent with graphical sessions.";
    };
  };

  config = lib.mkIf cfg.enable {
    environment.systemPackages = [ cfg.package ];
    services.udev.packages = [ cfg.package ];

    systemd.user.services.roadie-agent = {
      description = "OpenRoadie background agent";
      wantedBy = lib.optionals cfg.launchAtLogin [ "graphical-session.target" ];
      after = [ "graphical-session.target" ];
      partOf = lib.optionals cfg.launchAtLogin [ "graphical-session.target" ];

      serviceConfig = {
        ExecStart = lib.getExe' cfg.package "roadie-agent";
        Restart = "on-failure";
        RestartSec = 5;
      };
    };
  };
}
