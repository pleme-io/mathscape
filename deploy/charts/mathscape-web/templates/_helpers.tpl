{{/*
mathscape-web helpers — delegates to pleme-lib
*/}}

{{- define "mathscape-web.name" -}}
{{- include "pleme-lib.name" . -}}
{{- end }}

{{- define "mathscape-web.fullname" -}}
{{- include "pleme-lib.fullname" . -}}
{{- end }}

{{- define "mathscape-web.labels" -}}
{{- include "pleme-lib.labels" . -}}
{{- end }}

{{- define "mathscape-web.selectorLabels" -}}
{{- include "pleme-lib.selectorLabels" . -}}
{{- end }}

{{- define "mathscape-web.serviceAccountName" -}}
{{- include "pleme-lib.serviceAccountName" . -}}
{{- end }}
