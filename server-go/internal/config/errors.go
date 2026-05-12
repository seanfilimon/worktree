package config

import "errors"

var (
	ErrMissingTLSFiles        = errors.New("TLS is enabled but certificate or key file is missing")
	ErrProductionAuthRequired = errors.New("production requires bearer authentication with WT_SERVER_AUTH_TOKEN")
	ErrProductionIAMRequired  = errors.New("production requires policy IAM and cannot use allow-all-dev")
)
