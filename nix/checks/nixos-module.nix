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
  };
  testScript = ''
    machine.wait_for_unit("xuewen.service")
    machine.wait_for_open_port(8080)
    machine.succeed("curl -sf http://127.0.0.1:8080/api/stats | grep -q '\"total\"'")
    # The agent's startup preflight logs `agent ask: <problem>` for a
    # missing runner or missing node, and nothing at all when happy.
    machine.fail("journalctl -u xuewen.service | grep -q 'agent ask: '")
  '';
}
