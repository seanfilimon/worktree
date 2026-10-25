package server

import (
	"net"
	"net/http"
	"net/netip"
	"strings"
)

// forwardedFor rewrites r.RemoteAddr to the client address named by
// X-Forwarded-For when the connection comes from a trusted proxy. The
// rightmost address that is not itself a trusted proxy wins (proxies append
// to the header, so earlier entries are client-controlled). gRPC requests
// served through ServeHTTP see the rewritten address as their peer.
func forwardedFor(h http.Handler, trusted []netip.Prefix) http.Handler {
	if len(trusted) == 0 {
		return h
	}
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if client, ok := clientAddr(r, trusted); ok {
			r = r.Clone(r.Context())
			r.RemoteAddr = net.JoinHostPort(client.String(), "0")
		}
		h.ServeHTTP(w, r)
	})
}

// clientAddr returns the forwarded client address of r, if r comes from a
// trusted proxy and carries a usable X-Forwarded-For header.
func clientAddr(r *http.Request, trusted []netip.Prefix) (netip.Addr, bool) {
	isTrusted := func(a netip.Addr) bool {
		for _, p := range trusted {
			if p.Contains(a) {
				return true
			}
		}
		return false
	}
	host, _, err := net.SplitHostPort(r.RemoteAddr)
	if err != nil {
		host = r.RemoteAddr
	}
	peer, err := netip.ParseAddr(host)
	if err != nil || !isTrusted(peer.Unmap()) {
		return netip.Addr{}, false
	}
	var hops []string
	for _, v := range r.Header.Values("X-Forwarded-For") {
		hops = append(hops, strings.Split(v, ",")...)
	}
	for i := len(hops) - 1; i >= 0; i-- {
		a, err := netip.ParseAddr(strings.TrimSpace(hops[i]))
		if err != nil {
			return netip.Addr{}, false
		}
		a = a.Unmap()
		if !isTrusted(a) {
			return a, true
		}
	}
	return netip.Addr{}, false
}
