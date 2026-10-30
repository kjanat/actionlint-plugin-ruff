package wasm_test

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

	"actionlint.kjanat.dev/v2/analysis"
	"actionlint.kjanat.dev/v2/analyzer"
	"actionlint.kjanat.dev/v2/config"
	"actionlint.kjanat.dev/v2/plugin"
)

// The standalone wazero test exercises the frozen wire ABI independently.
// This test exercises the public actionlint loader and adapters consumers use.
func TestPackagedEngineThroughActionlint(t *testing.T) {
	directory := filepath.Dir(*wasmPath)
	metadata, err := os.ReadFile(filepath.Join(directory, "metadata.json"))
	if err != nil {
		t.Fatal(err)
	}
	var declaration struct {
		ID            string `json:"id"`
		Repository    string `json:"repository"`
		PluginVersion string `json:"plugin_version"`
	}
	if err := json.Unmarshal(metadata, &declaration); err != nil {
		t.Fatal(err)
	}
	if declaration.ID != "actionlint-plugin-ruff" ||
		declaration.Repository != "https://github.com/kjanat/actionlint-plugin-ruff" ||
		declaration.PluginVersion == "" {
		t.Fatalf("unexpected packaged identity: %+v", declaration)
	}
	checksums := packagedChecksums(t, directory)
	cache := t.TempDir()
	for _, filename := range []string{"plugin.wasm", "plugin.wasm.gz"} {
		t.Run(filename, func(t *testing.T) {
			ctx, cancel := context.WithTimeout(t.Context(), 2*time.Minute)
			defer cancel()
			path := filepath.Join(directory, filename)
			data, err := os.ReadFile(path)
			if err != nil {
				t.Fatal(err)
			}
			digest := sha256.Sum256(data)
			hexDigest := hex.EncodeToString(digest[:])
			if checksums[filename] != hexDigest {
				t.Fatalf("packaged bytes differ from SHA256SUMS: %s", hexDigest)
			}
			info, err := plugin.Inspect(ctx, path, plugin.ArtifactOptions{})
			if err != nil {
				t.Fatal(err)
			}
			if info.Kind != plugin.CoreArtifact || info.SHA256 != hexDigest || !bytes.Equal(info.Metadata, metadata) {
				t.Fatalf("public inspection lost artifact identity: kind=%s digest=%s", info.Kind, info.SHA256)
			}
			t.Logf("verified packaged %s@%s %s sha256:%s", declaration.ID, declaration.PluginVersion, filename, hexDigest)
			engine, err := plugin.LoadEngine(ctx, path, plugin.EngineOptions{CacheDir: cache, Timeout: time.Minute})
			if err != nil {
				t.Fatal(err)
			}
			defer func() {
				if err := engine.Close(context.Background()); err != nil {
					t.Errorf("close actionlint engine: %v", err)
				}
			}()
			runtimeMetadata := engine.Metadata()
			if runtimeMetadata.ID != declaration.ID || runtimeMetadata.PluginVersion != declaration.PluginVersion ||
				runtimeMetadata.ABIVersion != 1 || runtimeMetadata.Transport != "wasm-core-json" ||
				runtimeMetadata.Capabilities.PositionEncoding != "unicode" ||
				len(runtimeMetadata.ConfigSchema) == 0 {
				t.Fatalf("unexpected runtime metadata: %+v", runtimeMetadata)
			}
			var validator analyzer.ScriptConfigValidator = engine
			settings := json.RawMessage(`{"lint":{"select":["F821"]}}`)
			if err := validator.ValidateConfig(ctx, settings); err != nil {
				t.Fatalf("valid Ruff configuration rejected: %v", err)
			}
			if err := validator.ValidateConfig(ctx, json.RawMessage(`{"lint":{"select":["FAKE999"]}}`)); err == nil {
				t.Fatal("invalid Ruff selector accepted through public validator")
			}
			var backend analyzer.ScriptBackend = engine
			if engine.ScriptDescriptor().TemplateContract != analyzer.TemplateEffectsContract {
				t.Fatal("public core backend lost negotiated template contract")
			}
			findings, err := backend.Check(ctx, analyzer.ScriptRequest{
				Source: "label = 'é🦀'; print(missing) # noqa\r\n", Filename: "workflow.step.py", Config: settings,
			})
			if err != nil {
				t.Fatal(err)
			}
			if len(findings) != 1 {
				t.Fatalf("want one unsuppressed F821 finding: %+v", findings)
			}
			finding := findings[0]
			if finding.Code != "F821" || finding.TemplateEffect != analyzer.TemplateEffectOpaqueName || finding.Filename != "workflow.step.py" ||
				finding.Location != (analyzer.ScriptPosition{Row: 1, Column: 21}) ||
				finding.EndLocation != (analyzer.ScriptPosition{Row: 1, Column: 28}) ||
				!strings.Contains(finding.Message, "missing") {
				t.Fatalf("public backend changed Unicode range or diagnostic: %+v", finding)
			}
			checkWorkflowMapping(t, ctx, backend)
		})
	}
}

func TestPackagedEngineWithDefaultLimits(t *testing.T) {
	ctx, cancel := context.WithTimeout(t.Context(), time.Minute)
	defer cancel()
	start := time.Now()
	engine, err := plugin.LoadEngine(ctx, *wasmPath, plugin.EngineOptions{})
	t.Logf("default-limit cold load: %s", time.Since(start))
	if err != nil {
		t.Fatal(err)
	}
	defer func() {
		if err := engine.Close(context.Background()); err != nil {
			t.Errorf("close default-limit engine: %v", err)
		}
	}()
	start = time.Now()
	findings, err := engine.Check(ctx, analyzer.ScriptRequest{
		Source: "print(missing)\n", Filename: "step.py",
		Config: json.RawMessage(`{"lint":{"select":["F821"]}}`),
	})
	t.Logf("default-limit check: %s", time.Since(start))
	if err != nil || len(findings) != 1 || findings[0].Code != "F821" {
		t.Fatalf("default-limit result: %+v, %v", findings, err)
	}
}

func packagedChecksums(t *testing.T, directory string) map[string]string {
	t.Helper()
	data, err := os.ReadFile(filepath.Join(directory, "SHA256SUMS"))
	if err != nil {
		t.Fatal(err)
	}
	checksums := map[string]string{}
	for line := range strings.SplitSeq(strings.TrimSpace(string(data)), "\n") {
		fields := strings.Fields(line)
		if len(fields) != 2 {
			t.Fatalf("invalid package checksum line %q", line)
		}
		if _, exists := checksums[fields[1]]; exists {
			t.Fatalf("duplicate package checksum for %q", fields[1])
		}
		checksums[fields[1]] = fields[0]
	}
	return checksums
}

func checkWorkflowMapping(t *testing.T, ctx context.Context, backend analyzer.ScriptBackend) {
	t.Helper()
	cfg, err := config.Parse([]byte("tools:\n  ruff:\n    select: [F821]\n"))
	if err != nil {
		t.Fatal(err)
	}
	result, err := analysis.Analyze(ctx, analysis.Request{
		Sources: []analysis.Source{{Path: "ci.yml", Config: cfg, Content: []byte(
			"on: push\r\n" +
				"jobs:\r\n" +
				"  build:\r\n" +
				"    runs-on: ubuntu-latest\r\n" +
				"    steps:\r\n" +
				"      - shell: python\r\n" +
				"        run: |\r\n" +
				"          label = 'é🦀'; print(missing) # noqa\r\n" +
				"          print(${{ github.run_number }})\r\n",
		)}},
		Options: analysis.Options{Ruff: analysis.Tool{Backend: backend}},
	})
	if err != nil {
		t.Fatal(err)
	}
	count := 0
	for _, finding := range result.Diagnostics {
		if finding.Rule != "ruff" {
			continue
		}
		count++
		if finding.Rule != "ruff" || finding.Code != "F821" || finding.Path != "ci.yml" ||
			finding.Start.Line != 8 || finding.Start.Column != 31 ||
			finding.End.Line != 8 || finding.End.Column != 38 {
			t.Fatalf("public workflow adapter lost source range: %+v", finding)
		}
	}
	if count != 1 {
		t.Fatalf("want one mapped Ruff finding: %+v", result.Diagnostics)
	}
}
