{{/*
mathscape helpers — delegates to pleme-lib
*/}}

{{- define "mathscape.name" -}}
{{- include "pleme-lib.name" . -}}
{{- end }}

{{- define "mathscape.fullname" -}}
{{- include "pleme-lib.fullname" . -}}
{{- end }}

{{- define "mathscape.labels" -}}
{{- include "pleme-lib.labels" . -}}
{{- end }}

{{- define "mathscape.selectorLabels" -}}
{{- include "pleme-lib.selectorLabels" . -}}
{{- end }}

{{- define "mathscape.serviceAccountName" -}}
{{- include "pleme-lib.serviceAccountName" . -}}
{{- end }}

{{- define "mathscape.streamEnv" -}}
{{- $prefix := .prefix }}
{{- range $k, $v := .config }}
{{- $name := printf "%s__%s" $prefix (upper $k) }}
{{- if kindIs "map" $v }}
{{- include "mathscape.streamEnv" (dict "prefix" $name "config" $v) }}
{{- else }}
- name: {{ $name }}
  value: {{ ternary ($v | int64 | toString) ($v | toString) (and (kindIs "float64" $v) (eq (floor $v) $v)) | quote }}
{{- end }}
{{- end }}
{{- end }}
