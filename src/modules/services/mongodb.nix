{ pkgs, lib, config, ... }:

with lib;

let
  cfg = config.services.mongodb;

  # Authentication is enabled if a username is provided
  authEnabled = cfg.initDatabaseUsername != "";

  nodeDataDir = i:
    if cfg.replication.numNodes > 1
    then "$MONGODBDATA/node${toString i}"
    else "$MONGODBDATA";

  # Find if --port is in additionalArgs and get the value
  additionalPort =
    let
      findPort = list:
        if list == [ ] then null
        else if lib.head list == "--port" && lib.length list > 1 then lib.elemAt list 1
        else if lib.hasPrefix "--port=" (lib.head list) then lib.removePrefix "--port=" (lib.head list)
        else findPort (lib.tail list);
    in
    findPort cfg.additionalArgs;

  cleanAdditionalArgs =
    let
      filterArgs = list:
        if list == [ ] then [ ]
        else if lib.head list == "--port" then
          if lib.length list > 1 then filterArgs (lib.drop 2 list)
          else [ ]
        else if lib.hasPrefix "--port=" (lib.head list) then
          filterArgs (lib.tail list)
        else if lib.head list == "--auth" || lib.head list == "--noauth" then
          filterArgs (lib.tail list)
        else
          [ (lib.head list) ] ++ filterArgs (lib.tail list);
    in
    filterArgs cfg.additionalArgs;

  setupNodeScript = i:
    pkgs.writeShellScriptBin "setup-mongodb-${toString i}" ''
      set -euo pipefail
      DATA_DIR="${nodeDataDir i}"
      if [[ ! -d "$DATA_DIR" ]]; then
        mkdir -p "$DATA_DIR"
      fi

      if ${lib.boolToString (cfg.replication.enable && authEnabled)}; then
        # Shared keyfile for all nodes in the same MONGODBDATA root
        if [[ ! -f "$MONGODBDATA/keyfile" ]]; then
          mkdir -p "$MONGODBDATA"
          ${pkgs.openssl}/bin/openssl rand -base64 765 > "$MONGODBDATA/keyfile.tmp"
          mv -n "$MONGODBDATA/keyfile.tmp" "$MONGODBDATA/keyfile" || true
          rm -f "$MONGODBDATA/keyfile.tmp"
          chmod 400 "$MONGODBDATA/keyfile"
        fi
        if [[ "$MONGODBDATA/keyfile" != "$DATA_DIR/keyfile" ]]; then
          # Use install to copy and set permissions in one go
          install -m 400 "$MONGODBDATA/keyfile" "$DATA_DIR/keyfile"
        fi
      fi
    '';

  startNodeScript = i: procName:
    let
      replicationArgs =
        if cfg.replication.enable
        then
          (if authEnabled then [ "--keyFile" "${nodeDataDir i}/keyfile" ] else [ ])
          ++ [ "--replSet" cfg.replication.replSet ]
        else [ ];
      portArgs = [ "--port" (toString config.processes.${procName}.ports.db.value) ];
      bindArgs = [ "--bind_ip_all" ];
      authArgs = if authEnabled then [ "--auth" ] else [ "--noauth" ];
    in
    pkgs.writeShellScriptBin "start-mongodb-${toString i}" ''
      set -euo pipefail
      ${setupNodeScript i}/bin/setup-mongodb-${toString i}
      exec ${cfg.package}/bin/mongod ${
        lib.concatStringsSep " " (replicationArgs ++ portArgs ++ bindArgs ++ authArgs ++ cleanAdditionalArgs)
      } -dbpath "${nodeDataDir i}"
    '';

  configureScript =
    let
      members = lib.concatStringsSep ", " (map
        (i:
          let
            procName = if i == 1 then "mongodb" else "mongodb${toString i}";
            priority = if i == 1 then "2" else "0";
          in
          "{ _id: ${toString (i - 1)}, host: 'localhost:${toString config.processes.${procName}.ports.db.value}', priority: ${priority} }"
        )
        (lib.range 1 cfg.replication.numNodes));

      allNodes = map
        (i:
          let procName = if i == 1 then "mongodb" else "mongodb${toString i}"; in
          toString config.processes.${procName}.ports.db.value
        )
        (lib.range 1 cfg.replication.numNodes);
    in
    pkgs.writeShellScriptBin "configure-mongodb" ''
            set -euo pipefail

            # Helper to check if a node is reachable
            wait_for_node() {
              local port=$1
              echo "Waiting for node on port $port to be reachable..."
              until ${pkgs.mongosh}/bin/mongosh --port "$port" --eval "1" --quiet >/dev/null 2>&1; do
                sleep 1
              done
            }

            # Wait for all nodes to be up
            ${lib.concatStringsSep "\n" (map (port: "wait_for_node ${port}") allNodes)}

            echo "All nodes reachable. Waiting 2 seconds for stability..."
            sleep 2

            # We connect to the first node to configure the cluster
            PRIMARY_PORT="${toString config.processes.mongodb.ports.db.value}"

            if ${lib.boolToString cfg.replication.enable}; then
              REPL_CONFIG="{ _id: '${cfg.replication.replSet}', members: [ ${members} ] }"

              echo "Initiating/Reconfiguring replica-set if needed with config: $REPL_CONFIG"
              ${pkgs.mongosh}/bin/mongosh --port "$PRIMARY_PORT" --quiet --eval "
                try {
                  var status = rs.status();
                  if (status.ok) {
                    print(\"Replica set already initiated: \" + status.set);
                  } else {
                    print(\"Replica set status NOT ok: \" + JSON.stringify(status));
                  }
                } catch (e) {
                  var msg = (e.message || e.toString() || \"\").toLowerCase();
                  if (msg.includes(\"no replset config\") ||
                      msg.includes(\"invalid\") ||
                      msg.includes(\"is not a member\") ||
                      msg.includes(\"not_initiated\") ||
                      msg.includes(\"not initiated\")) {

                    print(\"Replica set needs attention. Attempting rs.initiate()...\");
                    var res = rs.initiate($REPL_CONFIG);
                    print(\"Initiate result: \" + JSON.stringify(res));

                    if (res.ok !== 1 && !JSON.stringify(res).includes(\"already initialized\")) {
                       // If initiate failed, try reconfig just in case it's in a weird state
                       print(\"Initiate didn't return OK. Trying reconfig as fallback...\");
                       var recRes = rs.reconfig($REPL_CONFIG, {force: true});
                       print(\"Reconfig result: \" + JSON.stringify(recRes));
                    }
                  } else {
                    print(\"Caught error during rs.status(): \" + msg);
                    throw e;
                  }
                }
              "

              echo "Waiting for a Primary to be elected (timeout 60s)..."
              # Wait up to 60 seconds for election
              for i in {1..30}; do
                if ${pkgs.mongosh}/bin/mongosh --port "$PRIMARY_PORT" --quiet --eval "db.hello().isWritablePrimary || db.hello().ismaster" | grep -q "true"; then
                  echo "Primary elected and writable."
                  break
                fi
                if [ $i -eq 30 ]; then
                  echo "Timeout waiting for primary election!"
                  exit 1
                fi
                sleep 2
              done
            fi

            if [ "${cfg.initDatabaseUsername}" ] && [ "${cfg.initDatabasePassword}" ]; then
                # Check if user already exists and can authenticate
                rootAuthDatabase="admin"
                if ${pkgs.mongosh}/bin/mongosh --port "$PRIMARY_PORT" --quiet "$rootAuthDatabase" -u "${cfg.initDatabaseUsername}" -p "${cfg.initDatabasePassword}" --authenticationDatabase "$rootAuthDatabase" --eval "1" >/dev/null 2>&1; then
                    echo "Initial user already exists and can authenticate."
                else
                    echo "Creating initial user..."
                    ${pkgs.mongosh}/bin/mongosh --port "$PRIMARY_PORT" --quiet "$rootAuthDatabase" >/dev/null <<-EOJS
                        db.createUser({
                            user: "${cfg.initDatabaseUsername}",
                            pwd: "${cfg.initDatabasePassword}",
                            roles: [ { role: 'root', db: "$rootAuthDatabase" } ]
                        })
      EOJS
                fi
            fi
    '';

in
{
  imports = [
    (lib.mkRenamedOptionModule [ "mongodb" "enable" ] [
      "services"
      "mongodb"
      "enable"
    ])
  ];

  options.services.mongodb = {
    enable = mkEnableOption "MongoDB process and expose utilities";

    package = mkOption {
      type = types.package;
      description = "Which MongoDB package to use.";
      default = pkgs.mongodb-ce;
      defaultText = lib.literalExpression "pkgs.mongodb-ce";
    };

    basePort = mkOption {
      type = types.port;
      default = 27017;
      description = "Base port for the MongoDB nodes. Devenv will find free ports starting from this.";
    };

    additionalArgs = lib.mkOption {
      type = types.listOf types.lines;
      default = [ "--noauth" ];
      example = [ "--port" "27017" "--noauth" ];
      description = ''
        Additional arguments passed to `mongod`. Note: --port, --dbpath, --replSet, --keyFile, --auth/--noauth are handled automatically.
      '';
    };

    replication = {
      enable = mkEnableOption "MongoDB replication.";
      numNodes = mkOption {
        type = types.int;
        default = 1;
        description = "Number of nodes in the replica-set.";
      };
      replSet = lib.mkOption {
        type = lib.types.str;
        default = "rs0";
        example = "rs0";
        description = "Replica-set name";
      };
    };

    initDatabaseUsername = lib.mkOption {
      type = types.str;
      default = "";
      example = "mongoadmin";
      description = ''
        Initial root user. Enabling this will also enable mandatory authentication and keyFile for replication when replication is enabled and auth is enabled.
      '';
    };

    initDatabasePassword = lib.mkOption {
      type = types.str;
      default = "";
      example = "secret";
      description = ''
        Password for the initial root user.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [ cfg.package pkgs.mongodb-tools pkgs.mongosh ];

    env.MONGODBDATA = config.env.DEVENV_STATE + "/mongodb";
    env.MONGODB_PORT = toString config.processes.mongodb.ports.db.value;

    processes = lib.listToAttrs (map
      (i:
        let procName = if i == 1 then "mongodb" else "mongodb${toString i}"; in
        {
          name = procName;
          value = {
            ports.db.allocate =
              if i == 1 && additionalPort != null
              then lib.toInt additionalPort
              else cfg.basePort + i - 1;
            exec = "${startNodeScript i procName}/bin/start-mongodb-${toString i}";
            ready.exec = "${pkgs.mongosh}/bin/mongosh --port ${toString config.processes.${procName}.ports.db.value} --quiet --eval \"1\"";
            # Secondary nodes wait for mongodb to be fully up and passing its own health check
            after = if i > 1 then [ "devenv:processes:mongodb" ] else [ ];
          };
        })
      (lib.range 1 cfg.replication.numNodes));

    tasks."mongodb:configure" = {
      exec = "${configureScript}/bin/configure-mongodb";
      after = map
        (i:
          let procName = if i == 1 then "mongodb" else "mongodb${toString i}"; in
          "devenv:processes:${procName}"
        )
        (lib.range 1 cfg.replication.numNodes);
    };
  };
}
