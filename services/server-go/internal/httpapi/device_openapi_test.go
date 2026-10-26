package httpapi

import "github.com/seanfilimon/worktree/services/server-go/internal/iam"

// device-auth (§16.6): register the component schemas of the device flow
// and refresh routes with the OpenAPI schema checks (kept in this file so
// that the shared openAPITypes table needs no edit).
func init() {
	for name, v := range map[string]any{
		"DeviceAuthorizationRequest":  deviceAuthorizationRequest{},
		"DeviceAuthorizationResponse": deviceAuthorizationResponse{},
		"DeviceTokenRequest":          deviceTokenRequest{},
		"DeviceApproveRequest":        deviceApproveRequest{},
		"DeviceRequestView":           iam.DeviceRequest{},
		"RefreshRequest":              refreshRequest{},
		"TokenPairResponse":           tokenPairResponse{},
	} {
		openAPITypes[name] = v
	}
	openAPICommentDirs = append(openAPICommentDirs, "../iam")
}
