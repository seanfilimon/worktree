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

{{/* "true" when the S3 settings are needed (object store or backups). */}}
{{- define "worktree.s3Used" -}}
{{- if or (eq .Values.objectStore "s3") (eq .Values.backup.target "s3") -}}true{{- end -}}
{{- end -}}

{{/* Name of the backup CronJob (CronJob names are limited to 52 characters). */}}
{{- define "worktree.backupCronJobName" -}}
{{- printf "%s-backup" (include "worktree.fullname" . | trunc 45 | trimSuffix "-") -}}
{{- end -}}

{{/*
A map entry as a string, "" when the key is absent (a null in the values
removes the key). Argument: (list MAP KEY).
*/}}
{{- define "worktree.optString" -}}
{{- $m := index . 0 -}}
{{- $k := index . 1 -}}
{{- if hasKey $m $k -}}{{- toString (index $m $k) -}}{{- end -}}
{{- end -}}

{{/* A PodDisruptionBudget bound: integers render bare, percentages quoted. */}}
{{- define "worktree.pdbValue" -}}
{{- if contains "%" (toString .) -}}{{ toString . | quote }}{{- else -}}{{ int . }}{{- end -}}
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
{{- if not (or (eq .Values.backup.target "disk") (eq .Values.backup.target "s3")) -}}
{{- fail "backup.target must be \"disk\" or \"s3\"" -}}
{{- end -}}
{{- if and (eq .Values.backup.target "s3") (not .Values.s3.bucket) -}}
{{- fail "backup.target=s3 requires s3.bucket" -}}
{{- end -}}
{{- if and .Values.backup.cronJob.enabled (not .Values.backup.cronJob.tokenSecret.name) -}}
{{- fail "backup.cronJob.enabled requires backup.cronJob.tokenSecret.name (a Secret holding a server-admin API token from `wt token create`)" -}}
{{- end -}}
{{- if .Values.podDisruptionBudget.enabled -}}
{{- $min := include "worktree.optString" (list .Values.podDisruptionBudget "minAvailable") -}}
{{- $max := include "worktree.optString" (list .Values.podDisruptionBudget "maxUnavailable") -}}
{{- if and $min $max -}}
{{- fail "podDisruptionBudget: set either minAvailable or maxUnavailable, not both (set maxUnavailable: null to use minAvailable)" -}}
{{- end -}}
{{- if not (or $min $max) -}}
{{- fail "podDisruptionBudget.enabled requires minAvailable or maxUnavailable" -}}
{{- end -}}
{{- end -}}
{{- end -}}
