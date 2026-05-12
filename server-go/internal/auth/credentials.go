package auth

import (
	"encoding/json"
	"fmt"
	"os"
)

type TokenCredentialFile struct {
	Tokens []TokenCredential `json:"tokens"`
}

func LoadTokenCredentialsFile(path string) ([]TokenCredential, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("read token credentials file: %w", err)
	}
	var file TokenCredentialFile
	if err := json.Unmarshal(data, &file); err != nil {
		return nil, fmt.Errorf("parse token credentials file: %w", err)
	}
	for i, token := range file.Tokens {
		if token.TokenID == "" {
			return nil, fmt.Errorf("token credentials file token %d: token_id is required", i)
		}
		if token.Secret == "" {
			return nil, fmt.Errorf("token credentials file token %q: secret is required", token.TokenID)
		}
		if token.Tenant == "" {
			return nil, fmt.Errorf("token credentials file token %q: tenant is required", token.TokenID)
		}
		if token.Account == "" {
			return nil, fmt.Errorf("token credentials file token %q: account is required", token.TokenID)
		}
	}
	return file.Tokens, nil
}
