{{/* Chart name. */}}
{{- define "ignition.name" -}}
{{- .Chart.Name | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{/* Release-qualified name, the usual fullname convention. */}}
{{- define "ignition.fullname" -}}
{{- if contains .Chart.Name .Release.Name -}}
{{- .Release.Name | trunc 63 | trimSuffix "-" -}}
{{- else -}}
{{- printf "%s-%s" .Release.Name .Chart.Name | trunc 63 | trimSuffix "-" -}}
{{- end -}}
{{- end -}}

{{/* Common labels. */}}
{{- define "ignition.labels" -}}
helm.sh/chart: {{ printf "%s-%s" .Chart.Name .Chart.Version }}
app.kubernetes.io/name: {{ include "ignition.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end -}}

{{/* Selector labels for a component; call with (dict "ctx" . "component" "web") */}}
{{- define "ignition.selectorLabels" -}}
app.kubernetes.io/name: {{ include "ignition.name" .ctx }}
app.kubernetes.io/instance: {{ .ctx.Release.Name }}
app.kubernetes.io/component: {{ .component }}
{{- end -}}

{{/* Image ref; call with (dict "ctx" . "img" .Values.web.image).

A repository containing "/" is a FULL image name and is used verbatim,
because that is what argocd-image-updater writes back when it pins a
digest; a bare name gets image.registry prepended. */}}
{{- define "ignition.image" -}}
{{- $tag := .img.tag | default .ctx.Chart.AppVersion -}}
{{- if contains "/" .img.repository -}}
{{- printf "%s:%s" .img.repository $tag -}}
{{- else -}}
{{- printf "%s/%s:%s" .ctx.Values.image.registry .img.repository $tag -}}
{{- end -}}
{{- end -}}
