![](observer.png)
# Fedimint Observer
Fedimint Observer is intended to become the "mempool.space for Fedimint". Due to the privacy properties of Fedimint it
won't be able to show concrete transaction flows, but transfers in and out of single federations are visible. By making
clear to users what is and isn't visible I hope to make Fedimint more transparent and thus more trustworthy.
Furthermore, I hope that it can inform developer decisions around improving privacy by having access to easily queryable
federation data to quantify possible privacy improvements.

Currently, Fedimint Observer consists of three sub-APIs: the Federation Observer, the Federation Inspector and a
small Nostr API (`/nostr/*`) for federation announcements and ratings.

## Federation Observer
This is likely the component that most people are actively exposed to. It is a server application that scans a list of
federations for their publicly available data (session log, announcements, …) and writes the data to a postgres
database. The data is then exposed via a REST API.

These APIs live under the `/federations` path, you can find all the endpoints in [`fmo_core/src/federation/mod.rs`](fmo_core/src/federation/mod.rs).
One example is the [`/federations`](https://observer.fedimint.org/api/federations) endpoint itself that returns a list
of all federations that are being observed.

This API is also the data source for the frontend that powers https://observer.fedimint.org. The frontend is hosted in
the `fmo_frontend_react` directory and is built with React and TypeScript.

When it comes to stability guarantees the Postgres database should always be migratable backwards-compatibly, so we
don't lose historic data. The API under `/federations` isn't stable at this point and I'd recommend to subscribing to
changes in Fedimint Observer if building against it.

## Federation Inspector
The lesser-known component is an API under the `/config` path it can be used to get a JSON-encoded version of the
federation config if you have an invite code. The first time it fetches the config from the federation using the invite
code, after that it will return a version cached in memory. The endpoints can be found
in [`fmo_core/src/config/mod.rs`](fmo_core/src/config/mod.rs).

This service is already used by [bitcoinmints.com](https://bitcoinmints.com/?tab=mints&showFedimint=true) and can thus
be considered kinda stable.

## Development
Fedimint Observer comes with a [nix](https://nixos.org/) development environment. You can enter it by running `nix develop`.
In there you can run a variety of `just` commands (to be called as `just <COMMAND> <ARGS...>`), the most important ones are:
* `check`: run `cargo check` on the entire workspace
* `pg_start` and `pg_stop`: start/stop a postgresql instance for local testing in the background
* `pg_backup` and `pg_restore`: in case you are building a DB migration it's useful to be able to reset the DB

## Deployment

I currently run the public instance at https://observer.fedimint.org using the following nix config:

```nix
{ lib, pkgs, fedimint-observer, system, ... }: let
   fmo = fedimint-observer.legacyPackages."${system}";
 in {
  systemd.services.fedimint-observer = {
    enable = true;
    wantedBy = [ "multi-user.target" ];
    environment = {
      FO_BIND = "127.0.0.1:5000";
      FO_DATABASE = "postgresql:///fmo?user=fmo";
    };
    serviceConfig = {
      # Holds `FO_ADMIN_AUTH=<long random secret>`, the admin password used to
      # add federations via curl. Values in `environment` end up world-readable
      # in the Nix store.
      EnvironmentFile = "/var/lib/secrets/fedimint-observer.env";
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
      # Built for the API under /api on the same host (fmo_frontend_react_default
      # calls http://localhost:3000/api from the visitor's browser)
      root = fmo.fmo_frontend_react "/api";
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
