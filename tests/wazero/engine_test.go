package wasm_test

import (
	"bytes"
	"compress/gzip"
	"context"
	"encoding/json"
	"flag"
	"io"
	"os"
	"slices"
	"testing"
	"time"

	"github.com/tetratelabs/wazero"
	"github.com/tetratelabs/wazero/api"
)

var wasmPath = flag.String("wasm", "../../artifacts/plugin.wasm.gz", "WASM artifact to verify (raw or gzip)")

func TestCoreModuleInGo(t *testing.T) {
	ctx, cancel := context.WithTimeout(t.Context(), time.Minute)
	defer cancel()
	runtime := wazero.NewRuntimeWithConfig(ctx, wazero.NewRuntimeConfig().WithMemoryLimitPages(4096).WithCloseOnContextDone(true))
	defer func() {
		if err := runtime.Close(context.Background()); err != nil {
			t.Errorf("close WASM runtime: %v", err)
		}
	}()
	wasm, err := os.ReadFile(*wasmPath)
	if err != nil {
		t.Fatal(err)
	}
	if len(wasm) >= 2 && wasm[0] == 0x1f && wasm[1] == 0x8b {
		reader, err := gzip.NewReader(bytes.NewReader(wasm))
		if err != nil {
			t.Fatal(err)
		}
		wasm, err = io.ReadAll(reader)
		if err != nil {
			t.Fatal(err)
		}
		if err := reader.Close(); err != nil {
			t.Fatal(err)
		}
		raw, err := os.ReadFile("../../artifacts/plugin.wasm")
		if err != nil {
			t.Fatal(err)
		}
		if !bytes.Equal(wasm, raw) {
			t.Fatal("gzip asset must decode to the exact raw module")
		}
	}
	compiled, err := runtime.CompileModule(ctx, wasm)
	if err != nil {
		t.Fatal(err)
	}
	if len(compiled.ImportedFunctions()) != 0 || len(compiled.ImportedMemories()) != 0 {
		t.Fatal("the core module must not import host functions or memory")
	}
	verifyV1Exports(t, compiled)
	guest, err := runtime.InstantiateModule(ctx, compiled, wazero.NewModuleConfig())
	if err != nil {
		t.Fatal(err)
	}
	version, err := guest.ExportedFunction("abi_version").Call(ctx)
	if err != nil || version[0] != 1 {
		t.Fatalf("ABI version: %v, %v", version, err)
	}

	invoke := func(operation string, input []byte) map[string]json.RawMessage {
		t.Helper()
		var args []uint64
		if input != nil {
			allocated, err := guest.ExportedFunction("alloc").Call(ctx, uint64(len(input)))
			if err != nil || allocated[0] == 0 {
				t.Fatalf("allocation: %v, %v", allocated, err)
			}
			pointer := uint32(allocated[0])
			defer free(t, ctx, guest, pointer)
			if !guest.Memory().Write(pointer, input) {
				t.Fatal("input memory write failed")
			}
			args = []uint64{uint64(pointer), uint64(len(input))}
		}
		result, err := guest.ExportedFunction(operation).Call(ctx, args...)
		if err != nil {
			t.Fatal(err)
		}
		pointer, length := uint32(result[0]>>32), uint32(result[0])
		defer free(t, ctx, guest, pointer)
		bytes, ok := guest.Memory().Read(pointer, length)
		if !ok {
			t.Fatal("output memory read failed")
		}
		var response map[string]json.RawMessage
		if err := json.Unmarshal(bytes, &response); err != nil {
			t.Fatal(err)
		}
		return response
	}

	metadata := invoke("metadata", nil)
	if string(metadata["abi_version"]) != "1" || len(metadata["config_schema"]) == 0 {
		t.Fatalf("missing metadata: %v", metadata)
	}
	for _, input := range []string{`{"lint":{"select":["F821"]}}`, `{"select":["FAKE999"]}`} {
		response := invoke("validate_config", []byte(input))
		want := "true"
		if input == `{"select":["FAKE999"]}` {
			want = "false"
		}
		if string(response["ok"]) != want {
			t.Fatalf("config response: %v", response)
		}
	}
	for range 30 {
		response := invoke("check", []byte(`{"source":"label = 'é🦀'; print(missing) # noqa\r\n","filename":"workflow.step.py","config":{"lint":{"select":["F821"]}}}`))
		if string(response["ok"]) != "true" {
			t.Fatalf("check failed: %v", response)
		}
		var diagnostics []struct {
			Code     string `json:"code"`
			Location struct {
				Row    int `json:"row"`
				Column int `json:"column"`
			} `json:"location"`
		}
		if err := json.Unmarshal(response["diagnostics"], &diagnostics); err != nil {
			t.Fatal(err)
		}
		if len(diagnostics) != 1 || diagnostics[0].Code != "F821" || diagnostics[0].Location.Row != 1 || diagnostics[0].Location.Column != 21 {
			t.Fatalf("unexpected diagnostic: %+v", diagnostics)
		}
	}
}

// Keep this fixture frozen: new operations may add exports, but v1 must stay callable.
func verifyV1Exports(t *testing.T, compiled wazero.CompiledModule) {
	t.Helper()
	data, err := os.ReadFile("../fixtures/abi-v1.json")
	if err != nil {
		t.Fatal(err)
	}
	var contract struct {
		Exports map[string]struct {
			Parameters []string `json:"parameters"`
			Results    []string `json:"results"`
		} `json:"exports"`
	}
	if err := json.Unmarshal(data, &contract); err != nil {
		t.Fatal(err)
	}
	types := func(names []string) []api.ValueType {
		result := make([]api.ValueType, len(names))
		for i, name := range names {
			switch name {
			case "i32":
				result[i] = api.ValueTypeI32
			case "i64":
				result[i] = api.ValueTypeI64
			default:
				t.Fatalf("unknown fixture type %q", name)
			}
		}
		return result
	}
	for name, signature := range contract.Exports {
		definition, ok := compiled.ExportedFunctions()[name]
		if !ok {
			t.Fatalf("missing frozen v1 export %q", name)
		}
		if !slices.Equal(definition.ParamTypes(), types(signature.Parameters)) || !slices.Equal(definition.ResultTypes(), types(signature.Results)) {
			t.Fatalf("changed v1 signature for %q: %v -> %v", name, definition.ParamTypes(), definition.ResultTypes())
		}
	}
}

func free(t *testing.T, ctx context.Context, guest api.Module, pointer uint32) {
	t.Helper()
	if _, err := guest.ExportedFunction("dealloc").Call(ctx, uint64(pointer)); err != nil {
		t.Fatal(err)
	}
}
