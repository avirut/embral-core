; Hooked into the generated installer.nsi via `bundle.windows.nsis.installerHooks`.
;
; Tauri includes this file before it inserts MUI_PAGE_WELCOME, and MUI only
; falls back to its stock welcome text when MUI_WELCOMEPAGE_TEXT is undefined
; — so defining it here replaces that text. The stock line tells people to
; close all other applications before continuing, which is advice a per-user
; install with no shared files does not need.
;
; Keep this file ASCII: the script is built with `Unicode true`, and NSIS
; reads an included file as ANSI unless it carries a UTF-8 BOM.
;
; No NSIS_HOOK_* macros are defined here on purpose. The template guards
; every one of its hook calls with !ifmacrodef, so a defines-only file is
; fine.

!define MUI_WELCOMEPAGE_TEXT "Meeting transcription and notes, on your machine.$\r$\n$\r$\n$_CLICK"
