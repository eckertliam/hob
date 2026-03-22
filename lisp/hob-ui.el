;;; hob-ui.el --- Buffer UI for hob -*- lexical-binding: t -*-

;;; Commentary:
;; Manages the *hob* output buffer, streaming token display,
;; tool call rendering, and diff/patch application UI.
;; All rendering callbacks invoked by hob-ipc-dispatch live here.

;;; Code:

(defconst hob--buffer-name "*hob*"
  "Name of the hob output buffer.")

(defun hob-ui--get-or-create-buffer ()
  "Return the hob UI buffer, creating it if necessary."
  (or (get-buffer hob--buffer-name)
      (with-current-buffer (generate-new-buffer hob--buffer-name)
        (hob-ui-mode)
        (current-buffer))))

(defun hob-ui--buffer-append (text)
  "Append TEXT to the hob UI buffer."
  (with-current-buffer (hob-ui--get-or-create-buffer)
    (let ((inhibit-read-only t))
      (goto-char (point-max))
      (insert text))))

(defun hob-ui-task-started (task-id prompt)
  "Called when task TASK-ID begins with PROMPT."
  (hob-ui--buffer-append
   (format "\n--- Task %s ---\n> %s\n\n" task-id prompt))
  (display-buffer (hob-ui--get-or-create-buffer)))

(defun hob-ui-append-token (task-id content)
  "Append streaming token CONTENT for TASK-ID."
  (hob-ui--buffer-append content))

(defun hob-ui-tool-call (task-id tool input)
  "Render a tool call for TASK-ID: TOOL with INPUT."
  (hob-ui--buffer-append
   (format "\n[tool: %s]\n" tool)))

(defun hob-ui-tool-result (task-id tool output)
  "Render tool result for TASK-ID: TOOL returned OUTPUT."
  (hob-ui--buffer-append
   (format "[result: %s]\n" (truncate-string-to-width (or output "") 120))))

(defun hob-ui-task-done (task-id)
  "Called when task TASK-ID completes."
  (hob-ui--buffer-append (format "\n--- Done (%s) ---\n" task-id)))

(defun hob-ui-task-error (task-id message)
  "Called when task TASK-ID fails with MESSAGE."
  (hob-ui--buffer-append (format "\n--- Error (%s): %s ---\n" task-id message)))

(define-derived-mode hob-ui-mode special-mode "Hob"
  "Major mode for the hob agent output buffer."
  (setq buffer-read-only t)
  (setq-local truncate-lines nil))

(provide 'hob-ui)
;;; hob-ui.el ends here
