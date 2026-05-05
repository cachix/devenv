{ pkgs, lib, ... }:

{
  services.rabbitmq = {
    enable = true;
    managementPlugin = { enable = true; };

    # macOS restricts `ps -o rss=` against hardened-runtime targets like
    # `beam.smp` when the caller is a non-admin service account (e.g. the
    # `_github-runner` user used by our self-hosted CI). RabbitMQ's default
    # memory monitor shells to `ps` and crashes the `rabbitmq_management_agent`
    # plugin during boot. Use Erlang's allocator accounting instead.
    configItems = lib.mkIf pkgs.stdenv.isDarwin {
      "vm_memory_calculation_strategy" = "allocated";
    };
  };
}
