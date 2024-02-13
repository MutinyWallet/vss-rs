{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

    crane = {
      url = "github:ipetkov/crane";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, crane, flake-utils, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
        };

        craneLib = crane.lib.${system};
        my-crate = craneLib.buildPackage {
          src = craneLib.cleanCargoSource (craneLib.path ./.);

          buildInputs = [
            pkgs.openssl
            pkgs.zlib
            pkgs.postgresql
            pkgs.gcc
            pkgs.gcc.cc.lib
          ] ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
            pkgs.libiconv
          ];
        };

        setupPostgresScript = pkgs.writeShellScript "setup-postgres" ''
          export PGDATA=$(mktemp -d)
          export PGSOCKETS=$(mktemp -d)
          ${pkgs.postgresql}/bin/initdb -D $PGDATA
          ${pkgs.postgresql}/bin/pg_ctl start -D $PGDATA -o "-h localhost -p 5432 -k $PGSOCKETS"
          until ${pkgs.postgresql}/bin/pg_isready -h localhost -p 5432; do sleep 1; done
          ${pkgs.postgresql}/bin/createuser -h localhost -p 5432 -s postgres
          ${pkgs.postgresql}/bin/psql -h localhost -p 5432 -c "CREATE USER \"vss_user\" WITH PASSWORD 'password';" -U postgres
          ${pkgs.postgresql}/bin/psql -h localhost -p 5432 -c "CREATE DATABASE \"vss\" OWNER \"vss_user\";" -U postgres
	  exit
        '';

        setupEnvScript = pkgs.writeShellScript "setup-env" ''
          if [ ! -f .env ]; then
            cp .env.sample .env
            sed -i 's|DATABASE_URL=postgres://localhost/vss|DATABASE_URL=postgres://vss_user:password@localhost:5432/vss|g' .env
          fi
        '';

      in
      {
        packages.default = my-crate;

        devShells.default = craneLib.devShell {
          inputsFrom = [ my-crate ];
          packages = [
            pkgs.openssl
            pkgs.zlib
            pkgs.postgresql
            pkgs.gcc
            pkgs.rust-analyzer
            pkgs.diesel-cli
            pkgs.gcc.cc.lib
          ];
          shellHook = ''
            export LD_LIBRARY_PATH=${pkgs.openssl}/lib:${pkgs.gcc.cc.lib}/lib:$LD_LIBRARY_PATH
            
            ${setupPostgresScript}
            ${setupEnvScript}
          '';
        };
      });
}
