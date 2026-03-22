;;; hob-ui.el --- Chat UI for hob -*- lexical-binding: t -*-

;;; Commentary:
;; Chat interface with read-only history above and an editable input area
;; below.  Streaming tokens, tool calls, permissions, and errors are
;; rendered into the history region with distinct faces.

;;; Code:

(require 'cl-lib)

;; ── Faces ──────────────────────────────────────────────────────────

(defface hob-user-face
  '((t :inherit font-lock-keyword-face :weight bold))
  "Face for user prompt headers."
  :group 'hob)

(defface hob-assistant-face
  '((t :inherit font-lock-function-name-face :weight bold))
  "Face for assistant response headers."
  :group 'hob)

(defface hob-tool-face
  '((t :inherit font-lock-type-face))
  "Face for tool call indicators."
  :group 'hob)

(defface hob-tool-result-face
  '((t :inherit font-lock-comment-face))
  "Face for tool results."
  :group 'hob)

(defface hob-error-face
  '((t :inherit error))
  "Face for error messages."
  :group 'hob)

(defface hob-status-face
  '((t :inherit font-lock-warning-face))
  "Face for status messages (retry, etc)."
  :group 'hob)

(defface hob-separator-face
  '((t :inherit shadow))
  "Face for separators and delimiters."
  :group 'hob)

(defface hob-prompt-face
  '((t :inherit minibuffer-prompt))
  "Face for the input prompt marker."
  :group 'hob)

;; ── Buffer state ───────────────────────────────────────────────────

(defconst hob--buffer-name "*hob*"
  "Name of the hob chat buffer.")

(defvar-local hob--input-marker nil
  "Marker at the start of the editable input area.")

(defvar-local hob--output-end nil
  "Marker at the end of the chat history (before the input separator).")

(defvar-local hob--current-task-id nil
  "ID of the currently running task, or nil.")

(defvar-local hob--input-history nil
  "List of previous input strings.")

(defvar-local hob--input-history-index -1
  "Current position in input history (-1 = not browsing).")

(defvar-local hob--following t
  "Non-nil if the buffer should auto-scroll to follow output.")

;; ── Buffer creation ────────────────────────────────────────────────

(defun hob-ui--get-or-create-buffer ()
  "Return the hob chat buffer, creating and initializing it if necessary."
  (or (get-buffer hob--buffer-name)
      (with-current-buffer (generate-new-buffer hob--buffer-name)
        (hob-chat-mode)
        (hob-ui--init-buffer)
        (current-buffer))))

(defun hob-ui--init-buffer ()
  "Set up the chat buffer with history region and input area."
  (let ((inhibit-read-only t))
    (erase-buffer)
    (insert (propertize "hob" 'face 'hob-assistant-face)
            (propertize " — AI coding agent.  "  'face 'hob-separator-face)
            (propertize "C-c C-k" 'face 'hob-prompt-face)
            (propertize " cancel  " 'face 'hob-separator-face)
            (propertize "C-c C-n" 'face 'hob-prompt-face)
            (propertize " new chat" 'face 'hob-separator-face)
            "\n"
            (propertize (make-string 60 ?─) 'face 'hob-separator-face)
            "\n\n")
    ;; Mark end of output region
    (setq hob--output-end (point-marker))
    (set-marker-insertion-type hob--output-end nil)
    ;; Insert input prompt
    (insert (propertize "\n" 'face 'hob-separator-face)
            (propertize (make-string 60 ?─) 'face 'hob-separator-face)
            "\n"
            (propertize "> " 'face 'hob-prompt-face))
    ;; Mark start of editable input
    (setq hob--input-marker (point-marker))
    (set-marker-insertion-type hob--input-marker t)
    ;; Make history region read-only
    (add-text-properties (point-min) hob--input-marker
                         '(read-only t rear-nonsticky t))))

;; ── Output (history region) ────────────────────────────────────────

(defun hob-ui--append-output (text &optional face)
  "Append TEXT to the chat history region, optionally with FACE."
  (with-current-buffer (hob-ui--get-or-create-buffer)
    (let ((inhibit-read-only t))
      (save-excursion
        (goto-char hob--output-end)
        (let ((start (point)))
          (insert text)
          (when face
            (add-text-properties start (point) `(face ,face)))
          ;; Keep everything above input read-only
          (add-text-properties start (point)
                               '(read-only t rear-nonsticky t)))))
    (when hob--following
      (hob-ui--scroll-to-bottom))))

(defun hob-ui--scroll-to-bottom ()
  "Scroll all windows showing the hob buffer to the bottom."
  (dolist (win (get-buffer-window-list (hob-ui--get-or-create-buffer) nil t))
    (with-selected-window win
      (goto-char hob--output-end)
      (recenter -3))))

;; ── IPC callbacks ──────────────────────────────────────────────────

(defun hob-ui-task-started (task-id prompt)
  "Called when task TASK-ID begins with PROMPT."
  (with-current-buffer (hob-ui--get-or-create-buffer)
    (setq hob--current-task-id task-id))
  (hob-ui--append-output (propertize "You:\n" 'face 'hob-user-face))
  (hob-ui--append-output (concat prompt "\n\n"))
  (hob-ui--append-output (propertize "hob:\n" 'face 'hob-assistant-face))
  (display-buffer (hob-ui--get-or-create-buffer)))

(defun hob-ui-append-token (_task-id content)
  "Append streaming token CONTENT."
  (hob-ui--append-output content))

(defun hob-ui-tool-call (_task-id tool _input)
  "Render a tool call: TOOL is being invoked."
  (hob-ui--append-output (format "\n  ● %s" tool) 'hob-tool-face))

(defun hob-ui-tool-result (_task-id _tool output)
  "Render tool result OUTPUT."
  (let ((short (truncate-string-to-width (or output "") 100)))
    (hob-ui--append-output (format "  ✓ %s\n" short) 'hob-tool-result-face)))

(defun hob-ui-task-done (task-id)
  "Called when task TASK-ID completes."
  (hob-ui--append-output
   (concat "\n" (propertize (make-string 40 ?─) 'face 'hob-separator-face) "\n\n"))
  (with-current-buffer (hob-ui--get-or-create-buffer)
    (setq hob--current-task-id nil)))

(defun hob-ui-task-error (_task-id message)
  "Called when a task fails with MESSAGE."
  (hob-ui--append-output (format "\n✗ %s\n\n" message) 'hob-error-face)
  (with-current-buffer (hob-ui--get-or-create-buffer)
    (setq hob--current-task-id nil)))

(defun hob-ui-task-status (_task-id message)
  "Called for status updates (retry, etc)."
  (hob-ui--append-output (format "\n  ⟳ %s\n" message) 'hob-status-face))

(defun hob-ui-permission-request (task-id request-id tool resource)
  "Prompt user for permission: TOOL wants to access RESOURCE."
  (hob-ui--append-output
   (format "\n  ⚠ %s: %s" tool resource) 'hob-status-face)
  (let ((choice (read-char-choice
                 (format " %s: %s (y=once, !=always, n=reject): " tool resource)
                 '(?y ?! ?n))))
    (let ((decision (pcase choice
                      (?y "once")
                      (?! "always")
                      (_ "reject"))))
      (hob-ipc-send-permission-response request-id decision)
      (hob-ui--append-output
       (format " → %s\n" decision)
       (if (equal decision "reject") 'hob-error-face 'hob-tool-result-face)))))

;; ── Input handling ─────────────────────────────────────────────────

(defun hob-ui--input-text ()
  "Return the current input text."
  (with-current-buffer (hob-ui--get-or-create-buffer)
    (string-trim
     (buffer-substring-no-properties hob--input-marker (point-max)))))

(defun hob-ui--clear-input ()
  "Clear the input area."
  (with-current-buffer (hob-ui--get-or-create-buffer)
    (let ((inhibit-read-only t))
      (delete-region hob--input-marker (point-max)))
    (goto-char hob--input-marker)))

(defun hob-ui-send ()
  "Send the current input to the agent."
  (interactive)
  (let ((input (hob-ui--input-text)))
    (when (string-empty-p input)
      (user-error "Nothing to send"))
    ;; Save to history
    (with-current-buffer (hob-ui--get-or-create-buffer)
      (push input hob--input-history)
      (setq hob--input-history-index -1)
      (setq hob--following t))
    (hob-ui--clear-input)
    ;; Start agent if needed and send
    (unless (hob-process-running-p)
      (hob-process-start))
    (hob-ipc-send-task input)))

(defun hob-ui-cancel ()
  "Cancel the current task."
  (interactive)
  (with-current-buffer (hob-ui--get-or-create-buffer)
    (if hob--current-task-id
        (progn
          (hob-ipc-send-cancel hob--current-task-id)
          (message "Cancelling task %s..." hob--current-task-id))
      (message "No task running"))))

(defun hob-ui-new-chat ()
  "Clear the chat and start fresh."
  (interactive)
  (when (and hob--current-task-id
             (yes-or-no-p "Task running.  Cancel and start new chat? "))
    (hob-ipc-send-cancel hob--current-task-id))
  (with-current-buffer (hob-ui--get-or-create-buffer)
    (let ((inhibit-read-only t))
      (hob-ui--init-buffer))
    (goto-char hob--input-marker)))

(defun hob-ui-history-prev ()
  "Replace input with the previous history entry."
  (interactive)
  (with-current-buffer (hob-ui--get-or-create-buffer)
    (when hob--input-history
      (setq hob--input-history-index
            (min (1+ hob--input-history-index)
                 (1- (length hob--input-history))))
      (let ((inhibit-read-only t))
        (delete-region hob--input-marker (point-max))
        (goto-char hob--input-marker)
        (insert (nth hob--input-history-index hob--input-history))))))

(defun hob-ui-history-next ()
  "Replace input with the next history entry, or clear."
  (interactive)
  (with-current-buffer (hob-ui--get-or-create-buffer)
    (setq hob--input-history-index
          (max -1 (1- hob--input-history-index)))
    (let ((inhibit-read-only t))
      (delete-region hob--input-marker (point-max))
      (goto-char hob--input-marker)
      (when (>= hob--input-history-index 0)
        (insert (nth hob--input-history-index hob--input-history))))))

(defun hob-ui-newline ()
  "Insert a literal newline in the input area."
  (interactive)
  (insert "\n"))

;; ── Scroll tracking ────────────────────────────────────────────────

(defun hob-ui--check-following ()
  "Update `hob--following' based on whether point is near the end."
  (when (eq (current-buffer) (get-buffer hob--buffer-name))
    (setq hob--following
          (>= (point) (- (point-max) 5)))))

;; ── Major mode ─────────────────────────────────────────────────────

(defvar hob-chat-mode-map
  (let ((map (make-sparse-keymap)))
    (define-key map (kbd "RET")       #'hob-ui-send)
    (define-key map (kbd "S-<return>") #'hob-ui-newline)
    (define-key map (kbd "C-j")       #'hob-ui-newline)
    (define-key map (kbd "C-c C-k")   #'hob-ui-cancel)
    (define-key map (kbd "C-c C-n")   #'hob-ui-new-chat)
    (define-key map (kbd "M-p")       #'hob-ui-history-prev)
    (define-key map (kbd "M-n")       #'hob-ui-history-next)
    map)
  "Keymap for `hob-chat-mode'.")

(define-derived-mode hob-chat-mode nil "Hob"
  "Major mode for the hob chat interface.

\\{hob-chat-mode-map}"
  (setq-local truncate-lines nil)
  (setq-local word-wrap t)
  (add-hook 'post-command-hook #'hob-ui--check-following nil t))

;; ── Legacy compat (hob-ui-mode alias) ──────────────────────────────

(defalias 'hob-ui-mode 'hob-chat-mode)

(provide 'hob-ui)
;;; hob-ui.el ends here
