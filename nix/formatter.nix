{
  flake,
  inputs,
  perSystem,
  pkgs,
  ...
}:
let
  rumdlConfig = (pkgs.formats.toml { }).generate "rumdl.toml" {
    MD013 = {
      reflow = true;
      reflow-mode = "sentence-per-line";
    };
  };
  # Author surnames (Ba), pdftotext small-caps fragments (R EFERENCES),
  # streamed-delta test text (Hel), a crate name (flate2), and the halves typos
  # splits mixed-case acronyms into (DELETEs, NAMEs, ANDed, PNGs).
  typosConfig = (pkgs.formats.toml { }).generate "typos.toml" {
    default.extend-words = pkgs.lib.genAttrs [
      "ba"
      "flate"
      "eferences"
      "hel"
      "mis"
      "unparseable"
      "empted"
      "delet"
      "nam"
      "ded"
      "pn"
    ] (word: word);
  };
  treefmt = inputs.treefmt-nix.lib.evalModule pkgs {
    projectRootFile = null;
    settings.global.excludes = [ "**/package-lock.json" ];

    programs.nixfmt.enable = true;
    programs.nixfmt.package = pkgs.nixfmt-rs;
    programs.deadnix.enable = true;
    programs.statix.enable = true;
    programs.rumdl-format.enable = true;
    programs.rustfmt.enable = true;
    programs.rustfmt.package = perSystem.fenix.stable.rustfmt;
    programs.rustfmt.edition = "2021";
    programs.taplo.enable = true;
    programs.typos.enable = true;
    programs.typos.configFile = "${typosConfig}";
    # Its "unambiguous" fixes still rewrite working code: it renamed a `pn`
    # binding to `on` and turned `PNGs` into `ONGs`.
    programs.typos.autoFix = false;
    programs.jsonfmt.enable = true;

    settings.formatter.rumdl-format.options = [
      "--config"
      "${rumdlConfig}"
    ];
  };
  formatter = treefmt.config.build.wrapper;
in
formatter
// {
  passthru = formatter.passthru // {
    tests.check = treefmt.config.build.check flake;
  };
}
