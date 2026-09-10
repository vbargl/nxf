package nixutil

import "testing"

const sampleListJSON = `{
  "elements": {
    "daily": {
      "active": true,
      "attrPath": "profileConfigurations.x86_64-linux.gui.daily",
      "originalUrl": "flake:vbargl",
      "url": "git+ssh://git.example.net/nixfiles?rev=abc",
      "priority": 5,
      "storePaths": ["/nix/store/aaa-profile-gui.daily"]
    },
    "daily-1": {
      "active": true,
      "attrPath": "profileConfigurations.x86_64-linux.terminal.daily",
      "originalUrl": "flake:vbargl",
      "url": "git+ssh://git.example.net/nixfiles?rev=abc",
      "priority": 4,
      "storePaths": ["/nix/store/bbb-profile-terminal.daily"]
    },
    "hello": {
      "active": true,
      "attrPath": "legacyPackages.x86_64-linux.hello",
      "originalUrl": "flake:nixpkgs",
      "url": "github:NixOS/nixpkgs",
      "priority": 5,
      "storePaths": ["/nix/store/ccc-hello-2.12"]
    }
  }
}`

func TestFindElementByStorePath(t *testing.T) {
	els, err := ParseElements([]byte(sampleListJSON))
	if err != nil {
		t.Fatalf("ParseElements: %v", err)
	}
	e, ok := FindElement(els, "gui.daily")
	if !ok || e.Name != "daily" {
		t.Fatalf("gui.daily -> %#v ok=%v, want Name=daily", e, ok)
	}
	e, ok = FindElement(els, "terminal.daily")
	if !ok || e.Name != "daily-1" {
		t.Fatalf("terminal.daily -> %#v ok=%v, want Name=daily-1", e, ok)
	}
	if _, ok = FindElement(els, "never-added"); ok {
		t.Fatal("never-added should not match")
	}
	if _, ok = FindElement(els, "daily"); ok {
		t.Fatal("bare 'daily' must not match profile-gui.daily / profile-terminal.daily")
	}
}

func TestFindElementByNewDerivationName(t *testing.T) {
	els, err := ParseElements([]byte(`{"elements":{
		"gui.daily":{"active":true,"priority":5,"storePaths":["/nix/store/ddd-gui.daily"]},
		"terminal.daily":{"active":true,"priority":5,"storePaths":["/nix/store/eee-terminal.daily"]}
	}}`))
	if err != nil {
		t.Fatal(err)
	}
	e, ok := FindElement(els, "gui.daily")
	if !ok || e.Name != "gui.daily" {
		t.Fatalf("gui.daily -> %#v ok=%v", e, ok)
	}
	e, ok = FindElement(els, "terminal.daily")
	if !ok || e.Name != "terminal.daily" {
		t.Fatalf("terminal.daily -> %#v ok=%v", e, ok)
	}
	if _, ok = FindElement(els, "daily"); ok {
		t.Fatal("bare daily must not match gui.daily or terminal.daily")
	}
}

func TestAttrPathName(t *testing.T) {
	cases := map[string]string{
		`profileConfigurations.x86_64-linux."gui.daily"`: "gui.daily",
		`profileConfigurations.x86_64-linux.gui.daily`:   "gui.daily",
		`profileConfigurations.x86_64-linux.media`:       "media",
		`legacyPackages.x86_64-linux.hello`:              "",
	}
	for in, want := range cases {
		if got := attrPathName(in); got != want {
			t.Errorf("attrPathName(%q) = %q, want %q", in, got, want)
		}
	}
}
