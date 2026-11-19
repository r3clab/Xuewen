# Needs KVM: `nix build .#checks.x86_64-linux.nixos-module -L`.
{ flake, pkgs, ... }:
pkgs.testers.runNixOSTest {
  name = "xuewen-module";
  meta.platforms = [ "x86_64-linux" ];
  nodes.machine = {
    imports = [ flake.nixosModules.default ];
    services.xuewen.enable = true;
    # Agent Ask on, so the test covers the runner wiring: both the
    # runner script and node have to resolve from the store, with
    # nothing installed under dataDir by hand.
    services.xuewen.settings.ai.agent.claude_code = { };
    environment.systemPackages = [ pkgs.jq ];
  };
  testScript = ''
    machine.wait_for_unit("xuewen.service")
    machine.wait_for_open_port(8080)
    machine.succeed("curl -sf http://127.0.0.1:8080/api/health | jq -e '.status == \"ok\"'")
    machine.succeed("curl -sf http://127.0.0.1:8080/api/stats | jq -e 'has(\"total\")'")
    machine.succeed("curl -sf http://127.0.0.1:8080/api/chat/models | jq -e '.available and any(.models[]; .id == \"claude_code\")'")
    machine.succeed("test -f /var/lib/xuewen/library.db")
    # The server listens only after the runner's preflight, which logs
    # `agent ask: <problem>` for a missing runner, node or SDK.
    machine.fail("journalctl -u xuewen.service | grep -q 'agent ask'")
  '';
}
