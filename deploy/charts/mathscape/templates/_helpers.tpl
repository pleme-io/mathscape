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
