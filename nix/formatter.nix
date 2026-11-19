{
  flake,
  inputs,
  pkgs,
  ...
}:
let
  inherit (pkgs) lib;
  rumdlConfig = (pkgs.formats.toml { }).generate "rumdl.toml" {
    MD013 = {
      reflow = true;
      reflow-mode = "sentence-per-line";
    };
  };
  typosConfig = (pkgs.formats.toml { }).generate "typos.toml" {
    default = {
      # Whole tokens only: an author surname (Ba), a pdftotext small-caps
      # fragment (EFERENCES), streamed-delta test text (Hel), GROBID's persName
      # binding (pn), and mixed-case acronym plurals typos would otherwise split
      # into fragments (ANDed, DELETEs, NAMEs, PNGs).
      extend-identifiers = lib.genAttrs [
        "ANDed"
        "Ba"
        "DELETEs"
        "EFERENCES"
        "Hel"
        "NAMEs"
        "PNGs"
        "pn"
      ] lib.id;
      # The spelling the code base uses; typos only knows "unparsable".
      extend-words.unparseable = "unparseable";
      # Hyphenated prefixes typos splits into a non-word ("pre-empted").
      extend-ignore-re = [
        "pre-empted"
        "\\bmis-[a-z]+"
      ];
    };
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
    programs.rustfmt.package = (flake.lib.rustToolchain pkgs).override { extensions = [ "rustfmt" ]; };
    programs.rustfmt.edition = (lib.importTOML ../Cargo.toml).workspace.package.edition;
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
