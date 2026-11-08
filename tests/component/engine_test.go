package component_test

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"actionlint.kjanat.dev/v2/internal/checking/scripts/registry"
	componenthost "actionlint.kjanat.dev/v2/internal/plugins/execution/component"

	"actionlint.kjanat.dev/v2/internal/checking"
	checkinputs "actionlint.kjanat.dev/v2/internal/checking/inputs"
	"actionlint.kjanat.dev/v2/internal/input/workspace"
	pluginartifact "actionlint.kjanat.dev/v2/internal/plugins/artifacts"
)

func TestPackagedScriptEngineComponent(t *testing.T) {
	const directory = "../../artifacts"
	metadata, err := os.ReadFile(filepath.Join(directory, "component.metadata.json"))
	if err != nil {
		t.Fatal(err)
	}
	var declaration struct {
		ID               string `json:"id"`
		PluginVersion    string `json:"plugin_version"`
		Repository       string `json:"repository"`
		Interface        string `json:"interface"`
		WITSHA256        string `json:"wit_sha256"`
		TemplateContract string `json:"template_contract"`
	}
	if err := json.Unmarshal(metadata, &declaration); err != nil {
		t.Fatal(err)
	}
	if declaration.ID != "actionlint-plugin-ruff" ||
		declaration.Repository != "https://github.com/kjanat/actionlint-plugin-ruff" ||
		declaration.Interface != "kjanat:actionlint/script-engine@0.2.0" {
		t.Fatalf("unexpected component identity: %+v", declaration)
	}
	if declaration.TemplateContract != checkinputs.TemplateEffectsContract {
		t.Fatalf("missing template semantics: %q", declaration.TemplateContract)
	}
	wit, err := os.ReadFile("../../vendor/actionlint-extension-api/wit/script-engine.wit")
	if err != nil {
		t.Fatal(err)
	}
	normalizedWIT := bytes.ReplaceAll(wit, []byte("\r\n"), []byte("\n"))
	contract := sha256.Sum256(normalizedWIT)
	if declaration.WITSHA256 != hex.EncodeToString(contract[:]) {
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
			inspected, err := pluginartifact.Resolve(ctx, path, pluginartifact.Options{})
			if err != nil {
				t.Fatal(err)
			}
			if inspected.Info.Kind != pluginartifact.Component || inspected.Info.SHA256 != digestHex || !bytes.Equal(inspected.Info.Metadata, metadata) {
				t.Fatal("public inspection lost component metadata or byte identity")
			}
			t.Logf("%s sha256:%s API:%s#sha256=%s", filename, digestHex, declaration.Interface, declaration.WITSHA256)
			engine, err := componenthost.Load(ctx, path, componenthost.Options{Timeout: time.Minute})
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
			if descriptor.TemplateContract != checkinputs.TemplateEffectsContract {
				t.Fatalf("host did not negotiate template effects: %+v", descriptor)
			}
			if descriptor.ID != declaration.ID || descriptor.Version != declaration.PluginVersion ||
				descriptor.Engine != "ruff" || descriptor.Rule.Name != "ruff" {
				t.Fatalf("unexpected script-engine descriptor: %+v", descriptor)
			}
			var validator checkinputs.ScriptConfigValidator = engine
			settings := json.RawMessage(`{"lint":{"select":["F821"]}}`)
			if err := validator.ValidateConfig(ctx, settings); err != nil {
				t.Fatal(err)
			}
			if err := validator.ValidateConfig(ctx, json.RawMessage(`{"lint":{"select":["FAKE999"]}}`)); err == nil {
				t.Fatal("component accepted invalid engine options")
			}
			var backend checkinputs.ScriptBackend = engine
			diagnostics, err := backend.Check(ctx, checkinputs.ScriptRequest{
				Source: "label = 'é🦀'; print(missing) # noqa\r\n", Filename: "step.py", Config: settings,
			})
			if err != nil || len(diagnostics) != 1 {
				t.Fatalf("component result: %+v, %v", diagnostics, err)
			}
			finding := diagnostics[0]
			if finding.TemplateEffect != checkinputs.TemplateEffectOpaqueName {
				t.Fatalf("typed semantic effect lost: %+v", finding)
			}
			if finding.Code != "F821" || finding.Filename != "step.py" ||
				finding.Location != (checkinputs.ScriptPosition{Row: 1, Column: 21}) ||
				finding.EndLocation != (checkinputs.ScriptPosition{Row: 1, Column: 28}) {
				t.Fatalf("component changed Unicode diagnostic range: %+v", finding)
			}
			diagnostics, err = backend.Check(ctx, checkinputs.ScriptRequest{
				Source: "print(1)\rprint(missing)\r", Filename: "step.py", Config: settings,
			})
			if err != nil || len(diagnostics) != 1 ||
				diagnostics[0].Location != (checkinputs.ScriptPosition{Row: 1, Column: 16}) ||
				diagnostics[0].EndLocation != (checkinputs.ScriptPosition{Row: 1, Column: 23}) {
				t.Fatalf("component rejected lone-CR source coordinates: %+v, %v", diagnostics, err)
			}
			diagnostics, err = backend.Check(ctx, checkinputs.ScriptRequest{
				Source: "def broken(\n", Filename: "step.py", Config: settings,
			})
			if err != nil || len(diagnostics) == 0 || diagnostics[0].Code != "invalid-syntax" {
				t.Fatalf("component lost syntax diagnostics: %+v, %v", diagnostics, err)
			}
			checkWorkflow(t, ctx, backend, "F821")
			checkWorkflow(t, ctx, presentationIndependentBackend{engine}, "renamed-check")
		})
	}
}

func checkWorkflow(t *testing.T, ctx context.Context, backend checkinputs.ScriptBackend, wantCode string) {
	t.Helper()
	cfg, err := checking.ConfigurationRules().Parse([]byte("tools:\n  ruff:\n    select: [F821]\n"))
	if err != nil {
		t.Fatal(err)
	}
	result, err := checking.Analyze(ctx, checking.Options{Tools: registry.Registrations(registry.Selections{"ruff": {Backend: backend}})},
		[]workspace.Source{{Path: "ci.yml", Config: cfg, Content: []byte(
			"on: push\r\njobs:\r\n  test:\r\n    runs-on: ubuntu-latest\r\n    steps:\r\n" +
				"      - shell: python\r\n        run: |\r\n" +
				"          label = 'é🦀'; print(missing) # noqa\r\n" +
				"          print(${{ github.run_number }})\r\n",
		)}})
	if err != nil {
		t.Fatalf("workflow analysis: %+v, %v", result, err)
	}
	count := 0
	for _, finding := range result.Diagnostics {
		if finding.Rule != "ruff" {
			continue
		}
		count++
		if finding.Rule != "ruff" || finding.Code != wantCode || finding.Path != "ci.yml" ||
			finding.Start.Line != 8 || finding.Start.Column != 31 || finding.End.Line != 8 || finding.End.Column != 38 {
			t.Fatalf("workflow diagnostic mapping changed: %+v", finding)
		}
	}
	if count != 1 {
		t.Fatalf("want one Ruff finding: %+v", result.Diagnostics)
	}
}

// Runtime negotiation validates the plugin contract. Changing package identity
// or display text must not change template handling or source mapping.
type presentationIndependentBackend struct{ *componenthost.Plugin }

func (b presentationIndependentBackend) ScriptDescriptor() checkinputs.ScriptEngineDescriptor {
	d := b.Plugin.ScriptDescriptor()
	d.ID, d.Rule.Provider = "example-independent-package", "example-independent-package"
	d.Name = "Different presentation"
	return d
}

func (b presentationIndependentBackend) Check(ctx context.Context, request checkinputs.ScriptRequest) ([]checkinputs.ScriptDiagnostic, error) {
	findings, err := b.Plugin.Check(ctx, request)
	for i := range findings {
		findings[i].Code = "renamed-check"
		findings[i].Name = "unrelated-name"
		findings[i].Message = "Translated presentation without generated identifiers"
	}
	return findings, err
}
