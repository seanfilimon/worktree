package config

import "errors"

var ErrMissingTLSFiles = errors.New("TLS is enabled but certificate or key file is missing")
