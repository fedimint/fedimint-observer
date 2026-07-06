![](observer.png)
# Fedimint Observer
Fedimint Observer is intended to become the "mempool.space for Fedimint". Due to the privacy properties of Fedimint it
won't be able to show concrete transaction flows, but transfers in and out of single federations are visible. By making
clear to users what is and isn't visible I hope to make Fedimint more transparent and thus more trustworthy.
Furthermore, I hope that it can inform developer decisions around improving privacy by having access to easily queryable
federation data to quantify possible privacy improvements.

Currently, Fedimint Observer consists of two sub-APIs:

## Federation Observer
This is likely the component that most people are actively exposed to. It is a server application that scans a list of
federations for their publicly available data (session log, announcements, …) and writes the data to a postgres
database. The data is then exposed via a REST API.

These APIs live under the `/federations` path, you can find all the endpoints in [`fmo_server/src/federation/mod.rs`](https://github.com/elsirion/fedimint-observer/blob/a7a540a9af9b6383b3f3a85b561241ca057baff5/fmo_server/src/federation/mod.rs#L27-L52).
One example is the [`/federations`](https://observer.fedimint.org/api/federations) endpoint itself that returns a list
of all federations that are being observed.

This API is also the data source for the frontend that powers https://observer.fedimint.org and is also hosted in this
repository in the `fmo_frontend` directory. The frontend is a Leptos PWA, so is fully written in Rust and compiled to
WASM. It uses [Tailwind](https://tailwindcss.com/) and [Flowbite](https://flowbite.com/) for styling.

When it comes to stability guarantees the Postgres database should always be migratable backwards-compatibly, so we
don't lose historic data. The API under `/federations` isn't stable at this point and I'd recommend to subscribing to
changes in Fedimint Observer if building against it.

## Federation Inspector
The lesser-known component is an API under the `/config` path it can be used to get a JSON-encoded version of the
federation config if you have an invite code. The first time it fetches the config from the federation using the invite
code, after that it will return a version cached in memory (till the service is restarted). The endpoints can be found
in [`fmo_server/src/config/mod.rs`](https://github.com/elsirion/fedimint-observer/blob/a7a540a9af9b6383b3f3a85b561241ca057baff5/fmo_server/src/config/mod.rs#L28-L46).

This service is already used by [bitcoinmints.com](https://bitcoinmints.com/?tab=mints&showFedimint=true) and can thus
be considered kinda stable.

## Development
Fedimint Observer comes with a [nix](https://nixos.org/) development environment. You can enter it by running `nix develop`.
In there you can run a variety of `just` commands (to be called as `just <COMMAND> <ARGS...>`), the most important ones are:
* `check`: run `cargo check` on the entire workspace
* `pg_start` and `pg_stop`: start/stop a postgresql instance for local testing in the background
* `pg_backup` and `pg_restore`: in case you are building a DB migration it's useful to be able to reset the DB
* `serve_frontend`: automatically rebuild the frontend on changes and serve it with `trunk`

## Deployment

I currently run the public instance at https://observer.fedimint.org using the following nix config:

```nix
{ lib, pkgs, fedimint-observer, system, ... }: let
   fmo = fedimint-observer.packages."${system}";
 in {
  systemd.services.fedimint-observer = {
    enable = true;
    wantedBy = [ "multi-user.target" ];
    environment = {
      FO_BIND = "127.0.0.1:5000";
      FO_DATABASE = "postgresql:///fmo?user=fmo";
      # Set to your admin password, used to add federations to be observed via curl
      FO_ADMIN_AUTH = ;
      # Seconds to wait between materialized view refreshes
      FO_VIEW_REFRESH_INTERVAL_SECS = "60";
      ALLOW_CONFIG_CORS = "true";
    };
    serviceConfig = {
      ExecStart = ''
        ${fmo.fmo_server}/bin/fmo_server
      '';
      User = "fmo";
      Group = "fmo";
      Restart = "always";
      RestartSec = "10s";
    };
  };

  services.postgresql = {
    enable = true;
    ensureDatabases = [ "fmo" ];
    ensureUsers = [
      { name = "fmo"; }
    ];
    initialScript = pkgs.writeText "backend-initScript" ''
      GRANT ALL PRIVILEGES ON DATABASE fmo TO fmo;
      \c fmo
      GRANT ALL ON SCHEMA public TO fmo;
    '';
  };

  services.nginx = {
    enable = true;
    virtualHosts."observer.fedimint.org" = {
      enableACME = true;
      forceSSL = true;
      root = fmo.fmo_frontend;
      locations."/" = {
        extraConfig = ''
          try_files $uri $uri/ /index.html;
        '';
      };
      locations."/api/" = {
        proxyPass = "http://127.0.0.1:5000/";
      };
    };
  };

  users.users."fmo" = {
    isSystemUser = true;
    group = "fmo";
  };
  users.groups."fmo" = {};
}
```
