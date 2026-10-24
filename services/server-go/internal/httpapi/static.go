package httpapi

import (
	"bytes"
	"errors"
	"html/template"
	"io/fs"
	"log/slog"
	"net/http"
	"os"
	"path"
	"path/filepath"
	"strings"
	"time"
)

// newAdminHandler serves the admin SPA build in dir under /admin/.
//
// Existing files are served as-is (hashed files under assets/ with a
// long-lived immutable cache policy); any other path falls back to
// index.html so client-side routes work on reload. When dir is empty or
// holds no index.html, every request gets a 404 page explaining how to
// build the panel.
func newAdminHandler(dir string, log *slog.Logger) http.Handler {
	if dir == "" {
		return missingAdmin("no admin directory configured (--admin-dir / WT_ADMIN_DIR)")
	}
	if st, err := os.Stat(filepath.Join(dir, "index.html")); err != nil || st.IsDir() {
		log.Warn("admin panel not available: index.html missing", slog.String("admin_dir", dir))
		return missingAdmin("index.html not found in the configured admin directory")
	}
	return &spaHandler{fsys: os.DirFS(dir)}
}

type spaHandler struct{ fsys fs.FS }

func (h *spaHandler) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	name := strings.TrimPrefix(path.Clean(r.URL.Path), "/admin")
	name = strings.TrimPrefix(name, "/")
	w.Header().Set("X-Content-Type-Options", "nosniff")
	if name != "" && name != "index.html" && fs.ValidPath(name) {
		if st, err := fs.Stat(h.fsys, name); err == nil && !st.IsDir() {
			if strings.HasPrefix(name, "assets/") {
				w.Header().Set("Cache-Control", "public, max-age=31536000, immutable")
			} else {
				w.Header().Set("Cache-Control", "no-cache")
			}
			http.ServeFileFS(w, r, h.fsys, name)
			return
		}
		// A missing static asset must not be answered with HTML.
		if strings.HasPrefix(name, "assets/") || path.Ext(name) != "" && path.Ext(name) != ".html" {
			http.NotFound(w, r)
			return
		}
	}
	h.serveIndex(w, r)
}

func (h *spaHandler) serveIndex(w http.ResponseWriter, r *http.Request) {
	// Read on every request so a rebuilt panel is picked up without a
	// restart; index.html is small.
	data, err := fs.ReadFile(h.fsys, "index.html")
	if err != nil {
		status := http.StatusInternalServerError
		if errors.Is(err, fs.ErrNotExist) {
			status = http.StatusNotFound
		}
		http.Error(w, http.StatusText(status), status)
		return
	}
	var mod time.Time
	if st, err := fs.Stat(h.fsys, "index.html"); err == nil {
		mod = st.ModTime()
	}
	w.Header().Set("Content-Type", "text/html; charset=utf-8")
	w.Header().Set("Cache-Control", "no-cache")
	http.ServeContent(w, r, "index.html", mod, bytes.NewReader(data))
}

var missingAdminTmpl = template.Must(template.New("missing").Parse(`<!doctype html>
<html lang="en"><head><meta charset="utf-8"><title>W0rkTree admin panel not built</title>
<meta name="viewport" content="width=device-width, initial-scale=1">
<style>body{font-family:system-ui,sans-serif;max-width:40rem;margin:4rem auto;padding:0 1rem;line-height:1.5;color:#1f2328}
code,pre{background:#f6f8fa;border-radius:6px}pre{padding:1rem;overflow-x:auto}code{padding:.1rem .3rem}</style>
</head><body>
<h1>Admin panel not available</h1>
<p>The W0rkTree server is running, but it has no admin panel build to serve ({{.}}).</p>
<p>Build the panel and point the server at it:</p>
<pre>cd apps/admin
npm install
npm run build
worktree-server --admin-dir apps/admin/dist</pre>
<p>Or set <code>WT_ADMIN_DIR=apps/admin/dist</code>. For development, run <code>npm run dev</code> in
<code>apps/admin</code> and open <a href="http://localhost:5173">http://localhost:5173</a>.</p>
<p>The REST API is available under <code>/api/v1</code>; health at <a href="/healthz">/healthz</a>.</p>
</body></html>
`))

func missingAdmin(reason string) http.Handler {
	var buf bytes.Buffer
	_ = missingAdminTmpl.Execute(&buf, reason)
	page := buf.Bytes()
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "text/html; charset=utf-8")
		w.Header().Set("Cache-Control", "no-store")
		w.WriteHeader(http.StatusNotFound)
		_, _ = w.Write(page)
	})
}
