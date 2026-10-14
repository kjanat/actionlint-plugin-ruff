package wasm_test

import (
	"bufio"
	"bytes"
	"encoding/binary"
	"encoding/json"
	"io"
	"os"
	"testing"
)

func TestStaticMetadataWithoutRuntime(t *testing.T) {
	file, err := os.Open("../../artifacts/plugin.wasm")
	if err != nil {
		t.Fatal(err)
	}
	defer func() {
		if err := file.Close(); err != nil {
			t.Errorf("close WASM artifact: %v", err)
		}
	}()
	reader := bufio.NewReader(file)
	header := make([]byte, 8)
	if _, err := io.ReadFull(reader, header); err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(header, []byte{0, 97, 115, 109, 1, 0, 0, 0}) {
		t.Fatal("invalid core WASM header")
	}
	id, err := reader.ReadByte()
	if err != nil || id != 0 {
		t.Fatalf("first section must be metadata: %d, %v", id, err)
	}
	size, err := binary.ReadUvarint(reader)
	if err != nil || size > 1024*1024+64 {
		t.Fatalf("invalid metadata section size: %d, %v", size, err)
	}
	section := make([]byte, int(size))
	if _, err := io.ReadFull(reader, section); err != nil {
		t.Fatal(err)
	}
	contents := bytes.NewReader(section)
	nameSize, err := binary.ReadUvarint(contents)
	if err != nil || nameSize > uint64(contents.Len()) {
		t.Fatalf("invalid custom section name size: %d, %v", nameSize, err)
	}
	name := make([]byte, int(nameSize))
	if _, err := io.ReadFull(contents, name); err != nil {
		t.Fatal(err)
	}
	if string(name) != "actionlint.plugin" {
		t.Fatalf("unexpected custom section: %q", name)
	}
	metadata, err := io.ReadAll(contents)
	if err != nil {
		t.Fatal(err)
	}
	expected, err := os.ReadFile("../../artifacts/metadata.json")
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(metadata, expected) {
		t.Fatal("custom section must equal metadata.json byte for byte")
	}
	var decoded struct {
		ABIVersion int `json:"abi_version"`
		Engine     struct {
			Name    string `json:"name"`
			Version string `json:"version"`
		} `json:"engine"`
		Schema map[string]json.RawMessage `json:"config_schema"`
	}
	if err := json.Unmarshal(metadata, &decoded); err != nil {
		t.Fatal(err)
	}
	if decoded.ABIVersion != 1 || decoded.Engine.Name != "ruff" || decoded.Engine.Version == "" || len(decoded.Schema["properties"]) == 0 {
		t.Fatalf("incomplete metadata: %+v", decoded)
	}
}
