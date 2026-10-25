{{/* Chart name. */}}
{{- define "worktree.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{/* Fully qualified app name (release-scoped, at most 63 characters). */}}
{{- define "worktree.fullname" -}}
{{- if .Values.fullnameOverride -}}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" -}}
{{- else -}}
{{- $name := default .Chart.Name .Values.nameOverride -}}
{{- if contains $name .Release.Name -}}
{{- .Release.Name | trunc 63 | trimSuffix "-" -}}
{{- else -}}
{{- printf "%s-%s" .Release.Name $name | trunc 63 | trimSuffix "-" -}}
{{- end -}}
{{- end -}}
{{- end -}}

{{- define "worktree.chart" -}}
{{- printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "worktree.selectorLabels" -}}
app.kubernetes.io/name: {{ include "worktree.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end -}}

{{- define "worktree.labels" -}}
helm.sh/chart: {{ include "worktree.chart" . }}
{{ include "worktree.selectorLabels" . }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
app.kubernetes.io/part-of: worktree
{{- end -}}

{{- define "worktree.serviceAccountName" -}}
{{- if .Values.serviceAccount.create -}}
{{- default (include "worktree.fullname" .) .Values.serviceAccount.name -}}
{{- else -}}
{{- default "default" .Values.serviceAccount.name -}}
{{- end -}}
{{- end -}}

{{/* Name of the Secret holding WT_ADMIN_PASSWORD and other credentials. */}}
{{- define "worktree.secretName" -}}
{{- default (include "worktree.fullname" .) .Values.existingSecret -}}
{{- end -}}

{{- define "worktree.image" -}}
{{- printf "%s:%s" .Values.image.repository (default .Chart.AppVersion .Values.image.tag) -}}
{{- end -}}

{{/* HTTP or HTTPS, depending on server-side TLS. */}}
{{- define "worktree.scheme" -}}
{{- if .Values.server.tls.enabled -}}HTTPS{{- else -}}HTTP{{- end -}}
{{- end -}}

{{/* Validates mutually dependent values; renders nothing. */}}
{{- define "worktree.validate" -}}
{{- if and .Values.server.tls.enabled (not .Values.server.tls.secretName) -}}
{{- fail "server.tls.enabled requires server.tls.secretName (a kubernetes.io/tls Secret)" -}}
{{- end -}}
{{- if and .Values.server.tls.clientCASecretName (not .Values.server.tls.enabled) -}}
{{- fail "server.tls.clientCASecretName requires server.tls.enabled" -}}
{{- end -}}
{{- if not (or (eq .Values.objectStore "disk") (eq .Values.objectStore "s3")) -}}
{{- fail "objectStore must be \"disk\" or \"s3\"" -}}
{{- end -}}
{{- if and (eq .Values.objectStore "s3") (not .Values.s3.bucket) -}}
{{- fail "objectStore=s3 requires s3.bucket" -}}
{{- end -}}
{{- if and .Values.persistence.existingClaim (not .Values.persistence.enabled) -}}
{{- fail "persistence.existingClaim requires persistence.enabled" -}}
{{- end -}}
{{- end -}}
