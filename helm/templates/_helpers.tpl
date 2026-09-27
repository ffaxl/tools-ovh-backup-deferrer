{{- define "deferrer.labels" -}}
app.kubernetes.io/name: {{ .Chart.Name }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end }}

{{- define "deferrer.secretName" -}}
{{- .Values.credentials.existingSecret | default .Release.Name }}
{{- end }}

{{- define "deferrer.image" -}}
{{- if .Values.image.digest -}}
{{ .Values.image.repository }}@{{ .Values.image.digest }}
{{- else -}}
{{ .Values.image.repository }}:{{ .Values.image.tag | default .Chart.AppVersion }}
{{- end }}
{{- end }}

{{- define "deferrer.services" -}}
{{- if not .Values.services }}
{{- fail "services must list at least one VPS" }}
{{- end }}
{{- $entries := list }}
{{- range .Values.services }}
{{- $entries = append $entries (printf "%s:%d" .name (int .offset)) }}
{{- end }}
{{- join "," $entries }}
{{- end }}
