;;; hob.el --- Native Emacs AI coding agent -*- lexical-binding: t -*-

;; Author: Your Name <you@example.com>
;; Version: 0.1.0
;; Package-Requires: ((emacs "29.1"))
;; Keywords: ai, tools, coding
;; URL: https://github.com/YOURUSERNAME/hob

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

(defcustom hob-api-key
  (or (getenv "ANTHROPIC_API_KEY") "")
  "Anthropic API key. Defaults to ANTHROPIC_API_KEY env var."
  :type 'string
  :group 'hob)

(defcustom hob-model "claude-opus-4-5"
  "Anthropic model to use."
  :type 'string
  :group 'hob)

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
