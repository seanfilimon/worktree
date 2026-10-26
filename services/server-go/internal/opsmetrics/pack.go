package opsmetrics

// pack-delta (docs/ENGINEERING_PLAN.md §16.1): pack transfer counters,
// incremented by syncsvc's UploadPack / DownloadPack. The direction label
// is "upload" (client to server) or "download".
var (
	PackBytes = NewCounter("worktree_pack_bytes_total",
		"Pack bytes transferred by direction (upload, download).", "direction")
	PackObjectBytes = NewCounter("worktree_pack_object_bytes_total",
		"Bytes of the objects carried by packs, by direction (what whole-object transfer would send).", "direction")
	PackBytesSaved = NewCounter("worktree_pack_bytes_saved_total",
		"Object bytes minus pack bytes (saved by deltas and compression), by direction.", "direction")
	PackDeltas = NewCounter("worktree_pack_deltas_total",
		"Objects transferred as deltas, by direction.", "direction")
	PackUploadResumes = NewCounter("worktree_pack_upload_resumes_total",
		"Pack uploads resumed at a non-zero offset.", "")
)

// packCounters lists the pack counters in rendering order.
func packCounters() []*Counter {
	return []*Counter{PackBytes, PackObjectBytes, PackBytesSaved, PackDeltas, PackUploadResumes}
}
