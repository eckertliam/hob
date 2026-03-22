;;; hob-ui.el --- Chat UI for hob -*- lexical-binding: t -*-

;;; Commentary:
;; Chat interface with read-only history above and an editable input area
;; below.  Streaming tokens, tool calls, permissions, and errors are
;; rendered into the history region with distinct faces.
;;
;; Features:
;; - Markdown rendering (code blocks, bold, italic, headers, inline code)
;; - Collapsible tool call/result sections (TAB to toggle)
;; - Modeline indicator showing agent state

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

(defface hob-md-bold-face
  '((t :weight bold))
  "Face for **bold** markdown text."
  :group 'hob)

(defface hob-md-italic-face
  '((t :slant italic))
  "Face for *italic* markdown text."
  :group 'hob)

(defface hob-md-code-face
  '((t :inherit fixed-pitch :background "gray20" :extend t))
  "Face for `inline code` in markdown."
  :group 'hob)

(defface hob-md-code-block-face
  '((t :inherit fixed-pitch :background "gray15" :extend t))
  "Face for fenced code blocks in markdown."
  :group 'hob)

(defface hob-md-heading-face
  '((t :inherit font-lock-function-name-face :weight bold :height 1.1))
  "Face for markdown headings."
  :group 'hob)

(defface hob-collapsed-face
  '((t :inherit shadow :slant italic))
  "Face for collapsed section indicators."
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

(defvar-local hob--agent-status "idle"
  "Current agent status for the modeline.")

(defvar-local hob--input-history nil
  "List of previous input strings.")

(defvar-local hob--input-history-index -1
  "Current position in input history (-1 = not browsing).")

(defvar-local hob--following t
  "Non-nil if the buffer should auto-scroll to follow output.")

(defvar-local hob--streaming-text ""
  "Accumulates streaming tokens for the current response.
Used to apply markdown rendering when the response completes.")

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
    ;; Mark end of output region (advances past inserts so appends go in order)
    (setq hob--output-end (point-marker))
    (set-marker-insertion-type hob--output-end t)
    ;; Insert input prompt
    (insert (propertize "\n" 'face 'hob-separator-face)
            (propertize (make-string 60 ?─) 'face 'hob-separator-face)
            "\n"
            (propertize "> " 'face 'hob-prompt-face))
    ;; Mark start of editable input (stays put so typed text falls after it)
    (setq hob--input-marker (point-marker))
    (set-marker-insertion-type hob--input-marker nil)
    ;; Make history region read-only
    (add-text-properties (point-min) hob--input-marker
                         '(read-only t rear-nonsticky t))
    ;; Reset state
    (setq hob--current-task-id nil
          hob--agent-status "idle"
          hob--streaming-text "")))

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

;; ── Markdown rendering ─────────────────────────────────────────────

(defun hob-ui--render-markdown (text)
  "Render TEXT with basic markdown formatting and insert into the output."
  (let ((rendered (hob-ui--fontify-markdown text)))
    (hob-ui--append-output rendered)))

(defun hob-ui--fontify-markdown (text)
  "Return a propertized copy of TEXT with markdown formatting applied."
  (with-temp-buffer
    (insert text)
    ;; Code blocks first (so they're protected from other transformations)
    (hob-ui--md-render-code-blocks)
    ;; Headers
    (hob-ui--md-render-headings)
    ;; Bold **text** and __text__
    (hob-ui--md-render-pattern "\\*\\*\\(.+?\\)\\*\\*" 'hob-md-bold-face)
    (hob-ui--md-render-pattern "__\\(.+?\\)__" 'hob-md-bold-face)
    ;; Italic *text* and _text_ (but not inside code or **)
    (hob-ui--md-render-pattern "\\(?:^\\|[^*_]\\)\\*\\([^*\n]+?\\)\\*\\(?:[^*]\\|$\\)"
                               'hob-md-italic-face)
    ;; Inline code `text`
    (hob-ui--md-render-inline-code)
    (buffer-string)))

(defun hob-ui--md-render-code-blocks ()
  "Render fenced code blocks (```...```) with code face."
  (goto-char (point-min))
  (while (re-search-forward "^```\\([a-zA-Z0-9_-]*\\)\n\\(\\(?:.*\n\\)*?\\)```$" nil t)
    (let ((lang (match-string 1))
          (code (match-string 2))
          (start (match-beginning 0))
          (end (match-end 0)))
      ;; Replace the fenced block with just the code, fontified
      (delete-region start end)
      (goto-char start)
      (let ((code-start (point)))
        (insert (hob-ui--fontify-code-block code lang))
        (add-text-properties code-start (point)
                             '(hob-code-block t))))))

(defun hob-ui--fontify-code-block (code lang)
  "Return CODE fontified for LANG if possible, with code-block face."
  (let ((mode (hob-ui--lang-to-mode lang)))
    (if mode
        (condition-case nil
            (with-temp-buffer
              (insert code)
              (delay-mode-hooks (funcall mode))
              (font-lock-ensure)
              (let ((result (buffer-string)))
                ;; Overlay the code-block background on top of syntax faces
                (add-face-text-property 0 (length result)
                                        'hob-md-code-block-face t result)
                result))
          (error
           (propertize code 'face 'hob-md-code-block-face)))
      (propertize code 'face 'hob-md-code-block-face))))

(defun hob-ui--lang-to-mode (lang)
  "Map a markdown language identifier LANG to an Emacs major mode, or nil."
  (cond
   ((or (null lang) (string-empty-p lang)) nil)
   ((member lang '("elisp" "emacs-lisp")) #'emacs-lisp-mode)
   ((member lang '("rust" "rs")) (and (fboundp 'rust-mode) #'rust-mode))
   ((member lang '("python" "py")) #'python-mode)
   ((member lang '("sh" "bash" "shell" "zsh")) #'sh-mode)
   ((member lang '("js" "javascript")) #'js-mode)
   ((member lang '("ts" "typescript")) (and (fboundp 'typescript-mode)
                                            #'typescript-mode))
   ((string= lang "c") #'c-mode)
   ((member lang '("cpp" "c++")) #'c++-mode)
   ((string= lang "go") (and (fboundp 'go-mode) #'go-mode))
   ((string= lang "ruby") #'ruby-mode)
   ((string= lang "json") #'js-mode)
   ((member lang '("yaml" "yml")) (and (fboundp 'yaml-mode) #'yaml-mode))
   ((string= lang "toml") (and (fboundp 'toml-mode) #'toml-mode))
   ((string= lang "sql") #'sql-mode)
   ((string= lang "html") #'html-mode)
   ((string= lang "css") #'css-mode)
   (t nil)))

(defun hob-ui--md-render-headings ()
  "Render # headings with heading face."
  (goto-char (point-min))
  (while (re-search-forward "^\\(#{1,4}\\) \\(.*\\)$" nil t)
    (unless (get-text-property (match-beginning 0) 'hob-code-block)
      (let ((heading (match-string 2))
            (start (match-beginning 0))
            (end (match-end 0)))
        (delete-region start end)
        (goto-char start)
        (insert (propertize heading 'face 'hob-md-heading-face))))))

(defun hob-ui--md-render-pattern (pattern face)
  "Apply FACE to text matching PATTERN (group 1 is the content)."
  (goto-char (point-min))
  (while (re-search-forward pattern nil t)
    (unless (get-text-property (match-beginning 0) 'hob-code-block)
      (let ((content (match-string 1))
            (start (match-beginning 0))
            (end (match-end 0)))
        ;; Only replace the matched delimiters, keep surrounding text
        (delete-region start end)
        (goto-char start)
        (insert (propertize content 'face face))))))

(defun hob-ui--md-render-inline-code ()
  "Render `inline code` with code face."
  (goto-char (point-min))
  (while (re-search-forward "`\\([^`\n]+\\)`" nil t)
    (unless (get-text-property (match-beginning 0) 'hob-code-block)
      (let ((code (match-string 1))
            (start (match-beginning 0))
            (end (match-end 0)))
        (delete-region start end)
        (goto-char start)
        (insert (propertize code 'face 'hob-md-code-face))))))

;; ── Collapsible sections ───────────────────────────────────────────

(defun hob-ui--insert-collapsible (header content &optional header-face content-face)
  "Insert a collapsible section with HEADER and hidden CONTENT.
HEADER-FACE applies to the header, CONTENT-FACE to the content.
TAB on the header line toggles visibility."
  (with-current-buffer (hob-ui--get-or-create-buffer)
    (let ((inhibit-read-only t))
      (save-excursion
        (goto-char hob--output-end)
        (let ((start (point))
              header-start header-end content-start content-end)
          ;; Insert header (always visible)
          (setq header-start (point))
          (insert (propertize header 'face (or header-face 'hob-tool-face)))
          (insert (propertize " ▸" 'face 'hob-collapsed-face))
          (setq header-end (point))
          ;; Insert content (initially hidden)
          (insert "\n")
          (setq content-start (point))
          (insert (propertize (or content "")
                              'face (or content-face 'hob-tool-result-face)))
          (setq content-end (point))
          (insert "\n")
          ;; Make content invisible
          (let ((ov (make-overlay content-start content-end)))
            (overlay-put ov 'invisible t)
            (overlay-put ov 'hob-collapsible t)
            (overlay-put ov 'isearch-open-invisible #'hob-ui--expand-overlay))
          ;; Mark the header as a toggle point
          (put-text-property header-start header-end
                             'hob-toggle-overlay t)
          ;; Read-only
          (add-text-properties start (point)
                               '(read-only t rear-nonsticky t))))))
  (when hob--following
    (hob-ui--scroll-to-bottom)))

(defun hob-ui--expand-overlay (ov)
  "Expand overlay OV (used by isearch)."
  (overlay-put ov 'invisible nil))

(defun hob-ui-toggle-section ()
  "Toggle the collapsible section at point."
  (interactive)
  (let ((ovs (overlays-at (save-excursion
                            (beginning-of-line)
                            (if (get-text-property (point) 'hob-toggle-overlay)
                                (1+ (line-end-position))
                              (point))))))
    ;; Also check overlays on the next line if we're on the header
    (when (get-text-property (point) 'hob-toggle-overlay)
      (setq ovs (append ovs (overlays-at (1+ (line-end-position))))))
    (dolist (ov ovs)
      (when (overlay-get ov 'hob-collapsible)
        (let ((inhibit-read-only t))
          (if (overlay-get ov 'invisible)
              (progn
                (overlay-put ov 'invisible nil)
                ;; Update arrow indicator
                (save-excursion
                  (goto-char (overlay-start ov))
                  (when (re-search-backward " [▸▾]" (line-beginning-position 0) t)
                    (replace-match " ▾"))))
            (overlay-put ov 'invisible t)
            (save-excursion
              (goto-char (overlay-start ov))
              (when (re-search-backward " [▸▾]" (line-beginning-position 0) t)
                (replace-match " ▸")))))))))

;; ── Modeline ───────────────────────────────────────────────────────

(defun hob-ui--modeline-string ()
  "Return the modeline string for hob status."
  (let ((status hob--agent-status))
    (cond
     ((string= status "idle")
      (propertize " hob:idle " 'face 'hob-separator-face))
     ((string= status "streaming")
      (propertize " hob:streaming● " 'face 'hob-assistant-face))
     ((string-prefix-p "tool:" status)
      (propertize (format " hob:%s " status) 'face 'hob-tool-face))
     ((string-prefix-p "retry" status)
      (propertize (format " hob:%s " status) 'face 'hob-status-face))
     ((string= status "waiting")
      (propertize " hob:permission? " 'face 'hob-status-face))
     (t
      (propertize (format " hob:%s " status) 'face 'hob-separator-face)))))

(defun hob-ui--set-status (status)
  "Set agent STATUS and update the modeline."
  (with-current-buffer (hob-ui--get-or-create-buffer)
    (setq hob--agent-status status)
    (force-mode-line-update t)))

;; ── IPC callbacks ──────────────────────────────────────────────────

(defun hob-ui-task-started (task-id prompt)
  "Called when task TASK-ID begins with PROMPT."
  (with-current-buffer (hob-ui--get-or-create-buffer)
    (setq hob--current-task-id task-id)
    (setq hob--streaming-text ""))
  (hob-ui--set-status "streaming")
  (hob-ui--append-output (propertize "You:\n" 'face 'hob-user-face
                                     'hob-message 'user))
  (hob-ui--append-output (concat prompt "\n\n"))
  (hob-ui--append-output (propertize "hob:\n" 'face 'hob-assistant-face
                                     'hob-message 'assistant))
  (display-buffer (hob-ui--get-or-create-buffer)))

(defun hob-ui-append-token (_task-id content)
  "Append streaming token CONTENT."
  (with-current-buffer (hob-ui--get-or-create-buffer)
    (setq hob--streaming-text (concat hob--streaming-text content)))
  (hob-ui--append-output content))

(defun hob-ui-tool-call (_task-id tool _input)
  "Render a tool call: TOOL is being invoked."
  ;; Render any accumulated markdown before the tool call
  (hob-ui--finalize-streaming)
  (hob-ui--set-status (format "tool:%s" tool))
  (hob-ui--append-output (format "\n  ● %s " tool) 'hob-tool-face))

(defun hob-ui-tool-result (_task-id tool output)
  "Render tool result OUTPUT as a collapsible section."
  (let ((short (truncate-string-to-width (or output "") 80))
        (full (or output "")))
    (hob-ui--insert-collapsible
     (format "  ✓ %s" short)
     (concat "    " (replace-regexp-in-string "\n" "\n    " full))
     'hob-tool-result-face
     'hob-tool-result-face))
  (hob-ui--set-status "streaming"))

(defun hob-ui-task-done (_task-id)
  "Called when a task completes."
  ;; Render accumulated markdown
  (hob-ui--finalize-streaming)
  (hob-ui--append-output
   (concat "\n" (propertize (make-string 40 ?─) 'face 'hob-separator-face) "\n\n"))
  (with-current-buffer (hob-ui--get-or-create-buffer)
    (setq hob--current-task-id nil))
  (hob-ui--set-status "idle"))

(defun hob-ui-task-error (_task-id message)
  "Called when a task fails with MESSAGE."
  (hob-ui--finalize-streaming)
  (hob-ui--append-output (format "\n✗ %s\n\n" message) 'hob-error-face)
  (with-current-buffer (hob-ui--get-or-create-buffer)
    (setq hob--current-task-id nil))
  (hob-ui--set-status "idle"))

(defun hob-ui-task-status (_task-id message)
  "Called for status updates (retry, etc)."
  (hob-ui--append-output (format "\n  ⟳ %s\n" message) 'hob-status-face)
  (hob-ui--set-status (if (string-match-p "retry" message) "retry" "busy")))

(defun hob-ui-permission-request (task-id request-id tool resource)
  "Prompt user for permission: TOOL wants to access RESOURCE."
  (hob-ui--finalize-streaming)
  (hob-ui--set-status "waiting")
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
       (if (equal decision "reject") 'hob-error-face 'hob-tool-result-face))
      (hob-ui--set-status "streaming"))))

;; ── Streaming finalization ─────────────────────────────────────────

(defun hob-ui--finalize-streaming ()
  "Replace the raw streamed text with markdown-rendered version.
Called when streaming pauses (tool call) or ends (done/error)."
  (with-current-buffer (hob-ui--get-or-create-buffer)
    (when (and (not (string-empty-p hob--streaming-text))
               (> (length hob--streaming-text) 0))
      (let ((text hob--streaming-text)
            (inhibit-read-only t))
        (setq hob--streaming-text "")
        ;; Find and delete the raw streamed text, replace with rendered
        (save-excursion
          (goto-char hob--output-end)
          ;; Search backward for the raw text
          (let ((end (point))
                (start (- (point) (length text))))
            (when (and (>= start (point-min))
                       (string= (buffer-substring-no-properties start end) text))
              (delete-region start end)
              (goto-char start)
              (let ((render-start (point))
                    (rendered (hob-ui--fontify-markdown text)))
                (insert rendered)
                (add-text-properties render-start (point)
                                     '(read-only t rear-nonsticky t))))))))))

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
      ;; Remove all overlays
      (remove-overlays (point-min) (point-max))
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

;; ── Message navigation ──────────────────────────────────────────────

(defun hob-ui-next-message ()
  "Jump to the next message header in the chat history."
  (interactive)
  (let ((pos (next-single-property-change (point) 'hob-message)))
    (when pos
      (when (get-text-property pos 'hob-message)
        (let ((next (next-single-property-change pos 'hob-message)))
          (when next (setq pos next))))
      (let ((target (text-property-any pos (point-max) 'hob-message 'user)))
        (unless target
          (setq target (text-property-any pos (point-max) 'hob-message 'assistant)))
        (when target
          (goto-char target)
          (recenter 2))))))

(defun hob-ui-prev-message ()
  "Jump to the previous message header in the chat history."
  (interactive)
  (let ((pos (previous-single-property-change (point) 'hob-message)))
    (when pos
      (let ((start (previous-single-property-change pos 'hob-message)))
        (when (and start (get-text-property start 'hob-message))
          (setq pos start)))
      (when (get-text-property pos 'hob-message)
        (goto-char pos)
        (recenter 2)))))

;; ── Scroll tracking ────────────────────────────────────────────────

(defun hob-ui--check-following ()
  "Update `hob--following' based on whether point is near the end."
  (when (eq (current-buffer) (get-buffer hob--buffer-name))
    (setq hob--following
          (>= (point) (- (point-max) 5)))))

;; ── Major mode ─────────────────────────────────────────────────────

(defvar hob-chat-mode-map
  (let ((map (make-sparse-keymap)))
    (define-key map (kbd "RET")        #'hob-ui-send)
    (define-key map (kbd "S-<return>") #'hob-ui-newline)
    (define-key map (kbd "C-j")        #'hob-ui-newline)
    (define-key map (kbd "C-c C-k")    #'hob-ui-cancel)
    (define-key map (kbd "C-c C-n")    #'hob-ui-new-chat)
    (define-key map (kbd "C-c C-p")    #'hob-ui-prev-message)
    (define-key map (kbd "C-c p")      #'hob-ui-prev-message)
    (define-key map (kbd "C-c C-f")    #'hob-ui-next-message)
    (define-key map (kbd "C-c n")      #'hob-ui-next-message)
    (define-key map (kbd "M-p")        #'hob-ui-history-prev)
    (define-key map (kbd "M-n")        #'hob-ui-history-next)
    (define-key map (kbd "TAB")        #'hob-ui-toggle-section)
    map)
  "Keymap for `hob-chat-mode'.")

(define-derived-mode hob-chat-mode nil "Hob"
  "Major mode for the hob chat interface.

\\{hob-chat-mode-map}"
  (setq-local truncate-lines nil)
  (setq-local word-wrap t)
  (setq mode-line-format
        (list "%e" 'mode-line-front-space
              '(:eval (hob-ui--modeline-string))
              " "
              'mode-line-buffer-identification
              "  " 'mode-line-position
              'mode-line-end-spaces))
  (add-hook 'post-command-hook #'hob-ui--check-following nil t))

;; ── Legacy compat ──────────────────────────────────────────────────

(defalias 'hob-ui-mode 'hob-chat-mode)

(provide 'hob-ui)
;;; hob-ui.el ends here
