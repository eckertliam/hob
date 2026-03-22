;;; hob.el --- Native Emacs AI coding agent -*- lexical-binding: t -*-

;; Author: Liam Eckert
;; Version: 0.1.0
;; Package-Requires: ((emacs "29.1"))
;; Keywords: ai, tools, coding
;; URL: https://github.com/eckertliam/hob

;;; Commentary:
;; hob is a native Emacs AI coding agent.
;; It drives a Rust subprocess (hob-agent) over stdio JSON IPC,
;; handling streaming responses, tool execution, and diff application
;; from within Emacs.

;;; Code:

(require 'hob-process)
(require 'hob-ipc)
(require 'hob-ui)

(defgroup hob nil
  "Native Emacs AI coding agent."
  :group 'tools
  :prefix "hob-")

(defcustom hob-agent-binary
  (expand-file-name
   "agent/target/release/hob-agent"
   (file-name-directory (or load-file-name buffer-file-name "")))
  "Path to the hob-agent Rust binary."
  :type 'file
  :group 'hob)

(defcustom hob-provider nil
  "LLM provider to use. nil means auto-detect from available API keys.
Set to \"anthropic\" or \"openai\" to force a provider."
  :type '(choice (const :tag "Auto-detect" nil)
                 (const :tag "Anthropic" "anthropic")
                 (const :tag "OpenAI" "openai"))
  :group 'hob)

(defcustom hob-api-key nil
  "API key for the selected provider.
If nil, the agent reads from ANTHROPIC_API_KEY or OPENAI_API_KEY env vars."
  :type '(choice (const :tag "Use environment variable" nil)
                 (string :tag "API key"))
  :group 'hob)

(defcustom hob-model "claude-sonnet-4-20250514"
  "Model to use. Examples:
  Anthropic: claude-sonnet-4-20250514, claude-opus-4-20250514
  OpenAI:    gpt-4o, gpt-4o-mini"
  :type 'string
  :group 'hob)

(defcustom hob-openai-base-url nil
  "Custom base URL for OpenAI-compatible APIs (e.g. local LLM servers).
If nil, uses the default OpenAI API URL."
  :type '(choice (const :tag "Default" nil)
                 (string :tag "Base URL"))
  :group 'hob)

;;;###autoload
(defun hob ()
  "Open the hob chat buffer."
  (interactive)
  (let ((buf (hob-ui--get-or-create-buffer)))
    (pop-to-buffer buf)
    (goto-char (point-max))))

;;;###autoload
(defun hob-start ()
  "Start the hob agent subprocess."
  (interactive)
  (hob-process-start))

;;;###autoload
(defun hob-stop ()
  "Stop the hob agent subprocess."
  (interactive)
  (hob-process-stop))

;;;###autoload
(defun hob-task (prompt)
  "Send PROMPT to hob as a new agent task."
  (interactive "sPrompt: ")
  (unless (hob-process-running-p)
    (hob-start))
  (hob-ipc-send-task prompt))

(provide 'hob)
;;; hob.el ends here
