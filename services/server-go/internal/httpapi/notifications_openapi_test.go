package httpapi

import (
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/notify"
)

// notifications (§15.3): register the component schemas of the
// notification routes with the OpenAPI schema checks (kept in this file so
// that the shared openAPITypes table needs no edit).
func init() {
	for name, v := range map[string]any{
		"Notification":                domain.Notification{},
		"WatchSetting":                domain.WatchSetting{},
		"NotificationList":            notify.ListResult{},
		"NotificationSettings":        notify.Settings{},
		"NotificationSettingsRequest": notify.SettingsPatch{},
		"WatchRequest":                notify.WatchPatch{},
		"WatchView":                   watchView{},
		"WatchList":                   watchList{},
		"ReadAllResult":               readAllResult{},
	} {
		openAPITypes[name] = v
	}
	openAPICommentDirs = append(openAPICommentDirs, "../notify")
}
