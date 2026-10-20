package component_test

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"actionlint.kjanat.dev/analysis"
	"actionlint.kjanat.dev/analyzer"
	"actionlint.kjanat.dev/config"
	"actionlint.kjanat.dev/plugin"
)

func TestPackagedScriptEngineComponent(t *testing.T) {
	const directory = "../../artifacts"
	metadata, err := os.ReadFile(filepath.Join(directory, "component.metadata.json"))
	if err != nil {
		t.Fatal(err)
	}
	var declaration struct {
		ID            string `json:"id"`
		PluginVersion string `json:"plugin_version"`
		Repository    string `json:"repository"`
		Interface     string `json:"interface"`
		Contract      string `json:"contract_sha256"`
	}
	if err := json.Unmarshal(metadata, &declaration); err != nil {
		t.Fatal(err)
	}
	if declaration.ID != "actionlint-plugin-ruff" ||
		declaration.Repository != "https://github.com/kjanat/actionlint-plugin-ruff" ||
		declaration.Interface != "kjanat:actionlint/script-engine@0.1.0" {
		t.Fatalf("unexpected component identity: %+v", declaration)
	}
	wit, err := os.ReadFile("../../crates/component/wit/script-engine.wit")
	if err != nil {
		t.Fatal(err)
	}
	normalizedWIT := bytes.ReplaceAll(wit, []byte("\r\n"), []byte("\n"))
	contract := sha256.Sum256(normalizedWIT)
	if declaration.Contract != hex.EncodeToString(contract[:]) {
		t.Fatal("packaged component does not identify the vendored WIT revision")
	}
	checksums, err := os.ReadFile(filepath.Join(directory, "SHA256SUMS"))
	if err != nil {
		t.Fatal(err)
	}
	for _, filename := range []string{"plugin.component.wasm", "plugin.component.wasm.gz"} {
		t.Run(filename, func(t *testing.T) {
			ctx, cancel := context.WithTimeout(t.Context(), 2*time.Minute)
			defer cancel()
			path := filepath.Join(directory, filename)
			content, err := os.ReadFile(path)
			if err != nil {
				t.Fatal(err)
			}
			digest := sha256.Sum256(content)
			digestHex := hex.EncodeToString(digest[:])
			if !strings.Contains(string(checksums), digestHex+"  "+filename+"\n") {
				t.Fatal("component bytes disagree with published checksums")
			}
			info, err := plugin.Inspect(ctx, path, plugin.ArtifactOptions{})
			if err != nil {
				t.Fatal(err)
			}
			if info.Kind != plugin.ComponentArtifact || info.SHA256 != digestHex || !bytes.Equal(info.Metadata, metadata) {
				t.Fatal("public inspection lost component metadata or byte identity")
			}
			t.Logf("%s sha256:%s API:%s#sha256=%s", filename, digestHex, declaration.Interface, declaration.Contract)
			engine, err := plugin.Load(ctx, path, plugin.Options{Timeout: time.Minute})
			if err != nil {
				t.Fatal(err)
			}
			defer func() {
				if err := engine.Close(context.Background()); err != nil {
					t.Errorf("close component: %v", err)
				}
			}()
			if !engine.HasScriptEngine() || engine.HasAnalyzer() || engine.HasRunnerProvider() {
				t.Fatal("component must expose only the script-engine role")
			}
			descriptor := engine.ScriptDescriptor()
			if descriptor.ID != declaration.ID || descriptor.Version != declaration.PluginVersion ||
				descriptor.Engine != "ruff" || descriptor.Rule.Name != "ruff" {
				t.Fatalf("unexpected script-engine descriptor: %+v", descriptor)
			}
			var validator analyzer.ScriptConfigValidator = engine
			settings := json.RawMessage(`{"lint":{"select":["F821"]}}`)
			if err := validator.ValidateConfig(ctx, settings); err != nil {
				t.Fatal(err)
			}
			if err := validator.ValidateConfig(ctx, json.RawMessage(`{"lint":{"select":["FAKE999"]}}`)); err == nil {
				t.Fatal("component accepted invalid engine options")
			}
			var backend analyzer.ScriptBackend = engine
			diagnostics, err := backend.Check(ctx, analyzer.ScriptRequest{
				Source: "label = 'é🦀'; print(missing) # noqa\r\n", Filename: "step.py", Config: settings,
			})
			if err != nil || len(diagnostics) != 1 {
				t.Fatalf("component result: %+v, %v", diagnostics, err)
			}
			finding := diagnostics[0]
			if finding.Code != "F821" || finding.Filename != "step.py" ||
				finding.Location != (analyzer.ScriptPosition{Row: 1, Column: 21}) ||
				finding.EndLocation != (analyzer.ScriptPosition{Row: 1, Column: 28}) {
				t.Fatalf("component changed Unicode diagnostic range: %+v", finding)
			}
			diagnostics, err = backend.Check(ctx, analyzer.ScriptRequest{
				Source: "print(1)\rprint(missing)\r", Filename: "step.py", Config: settings,
			})
			if err != nil || len(diagnostics) != 1 ||
				diagnostics[0].Location != (analyzer.ScriptPosition{Row: 2, Column: 7}) ||
				diagnostics[0].EndLocation != (analyzer.ScriptPosition{Row: 2, Column: 14}) {
				t.Fatalf("component rejected lone-CR source coordinates: %+v, %v", diagnostics, err)
			}
			diagnostics, err = backend.Check(ctx, analyzer.ScriptRequest{
				Source: "def broken(\n", Filename: "step.py", Config: settings,
			})
			if err != nil || len(diagnostics) == 0 || diagnostics[0].Code != "invalid-syntax" {
				t.Fatalf("component lost syntax diagnostics: %+v, %v", diagnostics, err)
			}
			checkWorkflow(t, ctx, backend)
		})
	}
}

func checkWorkflow(t *testing.T, ctx context.Context, backend analyzer.ScriptBackend) {
	t.Helper()
	cfg, err := config.Parse([]byte("tools:\n  ruff:\n    select: [F821]\n"))
	if err != nil {
		t.Fatal(err)
	}
	result, err := analysis.Analyze(ctx, analysis.Request{
		Sources: []analysis.Source{{Path: "ci.yml", Config: cfg, Content: []byte(
			"on: push\r\njobs:\r\n  test:\r\n    runs-on: ubuntu-latest\r\n    steps:\r\n" +
				"      - shell: python\r\n        run: |\r\n" +
				"          label = 'é🦀'; print(missing) # noqa\r\n" +
				"          print(${{ github.run_number }})\r\n",
		)}},
		Options: analysis.Options{Ruff: analysis.Tool{Backend: backend}},
	})
	if err != nil || len(result.Diagnostics) != 1 {
		t.Fatalf("workflow analysis: %+v, %v", result, err)
	}
	finding := result.Diagnostics[0]
	if finding.Rule != "ruff" || finding.Code != "F821" || finding.Path != "ci.yml" ||
		finding.Start.Line != 8 || finding.Start.Column != 31 || finding.End.Line != 8 || finding.End.Column != 38 {
		t.Fatalf("workflow diagnostic mapping changed: %+v", finding)
	}
}

func TestVendoredWITMatchesPinnedHost(t *testing.T) {
	// Resolve the exact published host source selected by this module.
	command := exec.CommandContext(t.Context(), "go", "list", "-m", "-f", "{{.Dir}}", "actionlint.kjanat.dev")
	output, err := command.Output()
	if err != nil {
		t.Fatalf("resolve pinned host source: %v", err)
	}
	host, err := os.ReadFile(filepath.Join(strings.TrimSpace(string(output)), "plugins", "wit", "script-engine.wit"))
	if err != nil {
		t.Fatal(err)
	}
	guest, err := os.ReadFile("../../crates/component/wit/script-engine.wit")
	if err != nil {
		t.Fatal(err)
	}
	normalize := func(data []byte) []byte { return bytes.ReplaceAll(data, []byte("\r\n"), []byte("\n")) }
	if !bytes.Equal(normalize(host), normalize(guest)) {
		t.Fatal("vendored script-engine WIT differs from the pinned host")
	}
}
