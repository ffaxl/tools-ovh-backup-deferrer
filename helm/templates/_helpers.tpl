{{- define "deferrer.labels" -}}
app.kubernetes.io/name: {{ .Chart.Name }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end }}

{{- define "deferrer.secretName" -}}
{{- required "credentialsSecret must name the secret holding the API keys" .Values.credentialsSecret }}
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
{{- $name := required "every entry in services needs a name" .name }}
{{- $offset := required (printf "service %s needs an offset" $name) .offset }}
{{- $entries = append $entries (printf "%s:%d" $name (int $offset)) }}
{{- end }}
{{- join "," $entries }}
{{- end }}

{{- define "deferrer.secretChecksum" -}}
{{- (lookup "v1" "Secret" .Release.Namespace (include "deferrer.secretName" .)).data | toJson | sha256sum }}
{{- end }}
