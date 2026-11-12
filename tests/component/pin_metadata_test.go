package component_test

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"os"
	"strings"
	"testing"

	"actionlint.kjanat.dev/v2/internal/plugins/artifacts"
	"actionlint.kjanat.dev/v2/internal/plugins/installation"
)

func TestRegistryPinAcceptsSDKWITChecksum(t *testing.T) {
	wit, err := os.ReadFile("../../vendor/actionlint-extension-api/wit/script-engine.wit")
	if err != nil {
		t.Fatal(err)
	}
	digest := sha256.Sum256([]byte(strings.ReplaceAll(string(wit), "\r\n", "\n")))
	checksum := hex.EncodeToString(digest[:])
	const iface = "kjanat:actionlint/script-engine@0.2.0"
	const artifactDigest = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
	pin := installation.Pin{
		Name: "github.com/kjanat/actionlint-plugin-ruff", PluginID: "actionlint-plugin-ruff", Version: "v0.2.0",
		Kind: artifacts.Component, SHA256: artifactDigest, Contract: iface + "#sha256=" + checksum,
	}
	metadata, err := json.Marshal(map[string]any{
		"metadata_version": 1, "id": pin.PluginID, "plugin_version": "0.2.0",
		"repository": "https://github.com/kjanat/actionlint-plugin-ruff", "transport": "wasm-component",
		"interface": iface, "wit_sha256": checksum,
	})
	if err != nil {
		t.Fatal(err)
	}
	info := artifacts.Info{Kind: artifacts.Component, SHA256: artifactDigest, Metadata: metadata}
	if err := pin.Verify(info); err != nil {
		t.Fatalf("SDK metadata rejected: %v", err)
	}
	pin.Contract = iface + "#sha256=" + strings.Repeat("b", 64)
	if err := pin.Verify(info); err == nil {
		t.Fatal("pin with a different WIT checksum was accepted")
	}
}
