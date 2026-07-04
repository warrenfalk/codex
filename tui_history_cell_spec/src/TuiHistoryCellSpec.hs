{-# LANGUAGE DuplicateRecordFields #-}
{-# LANGUAGE OverloadedStrings #-}

-- | Experimental event-sourced transcript specification for the Codex TUI.
--
-- Derived from repository commit f0c773b888d2a8e6ceac514f8741914d4bfb1b7e.
--
-- This module intentionally describes semantic transcript state, not ratatui rendering.
-- Wrapping, theme colors, animation frames, wall-clock timers, viewport dimensions, and file-opening
-- handles are renderer context and must stay outside 'TranscriptState'.
module TuiHistoryCellSpec
  ( ActiveCells (..),
    ApprovalKind (..),
    AssistantMessageCell (..),
    AssistantMessageDraft (..),
    ClientInteraction (..),
    CommandAction (..),
    CommandExecutionCell (..),
    CommandExecutionOutput (..),
    CommandExecutionSource (..),
    CommandStatus (..),
    DiagnosticReason (..),
    DisplayCell (..),
    FileChange (..),
    FileChangeKind (..),
    FileChangeStatus (..),
    GuardianDecision (..),
    HookCell (..),
    HookEntry (..),
    HookEntryKind (..),
    HookRun (..),
    HookRunId (..),
    HookStatus (..),
    ItemId (..),
    ImageGenerationNotice (..),
    Lifecycle (..),
    Millis,
    McpContentBlock (..),
    McpResult (..),
    McpToolCallCell (..),
    ModelVerification (..),
    NoticeCell (..),
    NoticeKind (..),
    PathText (..),
    PendingApproval (..),
    PendingInteractions (..),
    PendingUserInput (..),
    PlanCell (..),
    PlanStep (..),
    PlanStepStatus (..),
    ReasoningCell (..),
    ReasoningDraft (..),
    ReducerDiagnostic (..),
    RequestUserInputResultCell (..),
    RequestId (..),
    ServerNotification (..),
    ServerRequest (..),
    TextElement (..),
    ThreadId (..),
    ThreadItem (..),
    ThreadSnapshot (..),
    TranscriptInput (..),
    TranscriptState (..),
    Turn (..),
    TurnError (..),
    TurnErrorInfo (..),
    TurnId (..),
    TurnStatus (..),
    UrlText (..),
    UserInput (..),
    UserInputAnswer (..),
    UserInputOption (..),
    UserInputQuestion (..),
    UserMessageCell (..),
    WebSearchAction (..),
    WebSearchCell (..),
    emptyActiveCells,
    emptyPendingInteractions,
    emptyTranscriptState,
    reduce,
  )
where

import Data.Map.Strict (Map)
import qualified Data.Map.Strict as Map
import Data.Sequence (Seq, (|>))
import qualified Data.Sequence as Seq
import Data.Text (Text)
import qualified Data.Text as Text

newtype ThreadId = ThreadId Text
  deriving (Eq, Ord, Show)

newtype TurnId = TurnId Text
  deriving (Eq, Ord, Show)

newtype ItemId = ItemId Text
  deriving (Eq, Ord, Show)

newtype RequestId = RequestId Text
  deriving (Eq, Ord, Show)

newtype HookRunId = HookRunId Text
  deriving (Eq, Ord, Show)

newtype PathText = PathText Text
  deriving (Eq, Ord, Show)

newtype UrlText = UrlText Text
  deriving (Eq, Ord, Show)

type Millis = Int

-- | Decoded inputs that can affect event-sourced transcript state.
--
-- Transport and JSON-RPC correlation are deliberately outside this type. App-server request
-- responses are represented as 'ThreadSnapshotInput' only when their payload contains transcript
-- history relevant to display cells.
data TranscriptInput
  = ServerNotificationInput ServerNotification
  | ServerRequestInput ServerRequest
  | ThreadSnapshotInput ThreadSnapshot
  | ClientInteractionInput ClientInteraction
  deriving (Eq, Show)

-- | Event-sourced app/client actions needed to complete pending cells.
--
-- These are not app-server notifications, but they are still events. Without them, the reducer
-- cannot produce cells such as 'RequestUserInputResultCell'.
data ClientInteraction
  = ClientSubmittedUserInput RequestId [UserInputAnswer]
  | ClientInterruptedRequest RequestId
  | ClientResolvedApproval RequestId GuardianDecision
  deriving (Eq, Show)

data ThreadSnapshot = ThreadSnapshot
  { snapshotThreadId :: ThreadId,
    snapshotTurns :: [Turn]
  }
  deriving (Eq, Show)

data Turn = Turn
  { turnId :: TurnId,
    turnStatus :: TurnStatus,
    turnDurationMs :: Maybe Millis,
    turnItems :: [ThreadItem]
  }
  deriving (Eq, Show)

data TurnStatus
  = TurnInProgress
  | TurnCompleted
  | TurnFailed
  | TurnInterrupted
  deriving (Eq, Show)

-- | Decoded subset of app-server notifications that can affect the event-sourced transcript.
data ServerNotification
  = ThreadStarted ThreadSnapshot
  | TurnStarted ThreadId Turn
  | TurnCompletedNotification ThreadId Turn
  | ItemStarted ThreadId TurnId Millis ThreadItem
  | ItemCompleted ThreadId TurnId Millis ThreadItem
  | AgentMessageDelta ThreadId TurnId ItemId Text
  | PlanDelta ThreadId TurnId ItemId Text
  | ReasoningSummaryTextDelta ThreadId TurnId ItemId Int Text
  | ReasoningSummaryPartAdded ThreadId TurnId ItemId Int
  | ReasoningTextDelta ThreadId TurnId ItemId Int Text
  | CommandExecutionOutputDelta ThreadId TurnId ItemId Text
  | TerminalInteraction ThreadId TurnId ItemId Text Text
  | TurnPlanUpdated ThreadId TurnId (Maybe Text) [PlanStep]
  | HookStarted HookRun
  | HookCompleted HookRun
  | WarningNotice Text
  | GuardianWarningNotice Text
  | ConfigWarningNotice Text (Maybe Text)
  | ModelVerificationNotice [ModelVerification]
  | ErrorNotice TurnError
  | DeprecationNotice Text (Maybe Text)
  | GuardianApprovalReviewCompleted ThreadId TurnId GuardianDecision GuardianAction
  | ServerRequestResolved RequestId
  | IgnoredNotification Text
  deriving (Eq, Show)

data ServerRequest
  = CommandExecutionRequestApproval RequestId PendingApproval
  | FileChangeRequestApproval RequestId PendingApproval
  | PermissionsRequestApproval RequestId PendingApproval
  | McpServerElicitationRequest RequestId Text
  | ToolRequestUserInput RequestId PendingUserInput
  | UnsupportedServerRequest RequestId Text
  deriving (Eq, Show)

data ModelVerification
  = TrustedAccessForCyber
  | OtherModelVerification Text
  deriving (Eq, Show)

data TurnError = TurnError
  { errorMessage :: Text,
    errorInfo :: Maybe TurnErrorInfo,
    errorRetrying :: Bool
  }
  deriving (Eq, Show)

data TurnErrorInfo
  = CyberPolicy
  | ServerOverloaded
  | OtherTurnErrorInfo Text
  deriving (Eq, Show)

data ThreadItem
  = UserMessageItem ItemId [UserInput]
  | AgentMessageItem ItemId Text
  | ReasoningItem ItemId [Text] [Text]
  | PlanItem ItemId Text
  | CommandExecutionItem CommandExecutionCell
  | FileChangeItem ItemId FileChangeStatus [FileChange]
  | McpToolCallItem McpToolCallCell
  | WebSearchItem WebSearchCell
  | NoteToSelfItem ItemId Text
  | ImageViewItem ItemId PathText
  | ImageGenerationItem ItemId ImageGenerationNotice
  | CollabAgentToolCallItem ItemId Text
  | SubAgentActivityItem ItemId Text
  | EnteredReviewModeItem ItemId Text
  | ExitedReviewModeItem ItemId
  | ContextCompactionItem ItemId
  | UnknownThreadItem ItemId Text
  deriving (Eq, Show)

-- | Event-sourced transcript state.
--
-- 'transcriptCells' is ordered display history. 'activeCells' are live display cells that a
-- renderer should also draw, but which have not yet become stable transcript history.
-- 'pendingInteractions' is invisible reducer state, not transcript output.
data TranscriptState = TranscriptState
  { transcriptCells :: Seq DisplayCell,
    activeCells :: ActiveCells,
    pendingInteractions :: PendingInteractions,
    diagnostics :: Seq ReducerDiagnostic
  }
  deriving (Eq, Show)

data ActiveCells = ActiveCells
  { activeAssistantMessages :: Map ItemId AssistantMessageDraft,
    activeReasoning :: Map ItemId ReasoningDraft,
    activePlans :: Map ItemId PlanCell,
    activeCommands :: Map ItemId CommandExecutionCell,
    activeMcpCalls :: Map ItemId McpToolCallCell,
    activeWebSearches :: Map ItemId WebSearchCell,
    activeHooks :: Map HookRunId HookCell
  }
  deriving (Eq, Show)

data PendingInteractions = PendingInteractions
  { pendingUserInputs :: Map RequestId PendingUserInput,
    pendingApprovals :: Map RequestId PendingApproval,
    pendingMcpElicitations :: Map RequestId Text
  }
  deriving (Eq, Show)

emptyTranscriptState :: TranscriptState
emptyTranscriptState =
  TranscriptState
    { transcriptCells = Seq.empty,
      activeCells = emptyActiveCells,
      pendingInteractions = emptyPendingInteractions,
      diagnostics = Seq.empty
    }

emptyActiveCells :: ActiveCells
emptyActiveCells =
  ActiveCells
    { activeAssistantMessages = Map.empty,
      activeReasoning = Map.empty,
      activePlans = Map.empty,
      activeCommands = Map.empty,
      activeMcpCalls = Map.empty,
      activeWebSearches = Map.empty,
      activeHooks = Map.empty
    }

emptyPendingInteractions :: PendingInteractions
emptyPendingInteractions =
  PendingInteractions
    { pendingUserInputs = Map.empty,
      pendingApprovals = Map.empty,
      pendingMcpElicitations = Map.empty
    }

-- | Semantic cells, not one-to-one Rust implementation cells.
--
-- Rust local-only families intentionally excluded here include SessionHeaderHistoryCell,
-- SessionInfoCell, TooltipHistoryCell, StatusHistoryCell, TokenActivityHistoryCell,
-- UpdateAvailableHistoryCell, WebHyperlinkHistoryCell, and local CompositeHistoryCell uses.
data DisplayCell
  = UserMessageDisplay UserMessageCell
  | AssistantMessageDisplay AssistantMessageCell
  | ReasoningDisplay ReasoningCell
  | PlanDisplay PlanCell
  | PlanUpdateDisplay (Maybe Text) [PlanStep]
  | CommandExecutionDisplay CommandExecutionCell
  | PatchDisplay ItemId FileChangeStatus [FileChange]
  | McpToolCallDisplay McpToolCallCell
  | McpImageOutputMarkerDisplay ItemId
  | WebSearchDisplay WebSearchCell
  | HookDisplay HookCell
  | NoticeDisplay NoticeCell
  | RequestUserInputResultDisplay RequestUserInputResultCell
  deriving (Eq, Show)

-- Rust render family: UserHistoryCell.
-- Event provenance: ServerNotification::ItemCompleted.item.UserMessage.content[];
-- ServerNotification::TurnCompleted.turn.items[].UserMessage.content[]; thread snapshot items.
data UserMessageCell = UserMessageCell
  { userMessageText :: Text,
    userTextElements :: [TextElement],
    userRemoteImageUrls :: [UrlText]
  }
  deriving (Eq, Show)

data UserInput
  = TextInput Text [TextElement]
  | RemoteImageInput UrlText
  deriving (Eq, Show)

data TextElement = TextElement
  { textElementStart :: Int,
    textElementEnd :: Int,
    textElementKind :: Text
  }
  deriving (Eq, Show)

-- Rust render families: AgentMarkdownCell, AgentMessageCell, StreamingAgentTailCell.
-- Event provenance: ServerNotification::AgentMessageDelta.delta;
-- ServerNotification::ItemCompleted.item.AgentMessage.text; thread snapshot AgentMessage items.
data AssistantMessageCell = AssistantMessageCell
  { assistantMarkdown :: Text
  }
  deriving (Eq, Show)

data AssistantMessageDraft = AssistantMessageDraft
  { assistantDraftItemId :: ItemId,
    assistantDraftMarkdown :: Text
  }
  deriving (Eq, Show)

-- Rust render family: ReasoningSummaryCell.
-- Event provenance: ServerNotification::ReasoningSummaryTextDelta.delta;
-- ServerNotification::ReasoningTextDelta.delta; ReasoningSummaryPartAdded;
-- ServerNotification::ItemCompleted.item.Reasoning.summary/content; thread snapshot Reasoning items.
data ReasoningCell = ReasoningCell
  { reasoningSummaryMarkdown :: Text,
    reasoningRawContent :: [Text],
    reasoningTranscriptOnly :: Bool
  }
  deriving (Eq, Show)

data ReasoningDraft = ReasoningDraft
  { reasoningDraftItemId :: ItemId,
    reasoningDraftSummaryParts :: [Text],
    reasoningDraftRawParts :: [Text]
  }
  deriving (Eq, Show)

-- Rust render families: ProposedPlanCell, ProposedPlanStreamCell, StreamingPlanTailCell.
-- Event provenance: ServerNotification::PlanDelta.delta;
-- ServerNotification::ItemCompleted.item.Plan.text; thread snapshot Plan items.
data PlanCell = PlanCell
  { planItemId :: ItemId,
    planMarkdown :: Text,
    planLifecycle :: Lifecycle
  }
  deriving (Eq, Show)

-- Rust render family: PlanUpdateCell.
-- Event provenance: ServerNotification::TurnPlanUpdated.explanation/plan[].
data PlanStep = PlanStep
  { planStepText :: Text,
    planStepStatus :: PlanStepStatus
  }
  deriving (Eq, Show)

data PlanStepStatus
  = PlanPending
  | PlanInProgress
  | PlanCompleted
  deriving (Eq, Show)

-- Rust render family: ExecCell.
-- Event provenance: ServerNotification::ItemStarted.item.CommandExecution;
-- ServerNotification::CommandExecutionOutputDelta.delta;
-- ServerNotification::ItemCompleted.item.CommandExecution; thread snapshot CommandExecution items.
data CommandExecutionCell = CommandExecutionCell
  { commandItemId :: ItemId,
    commandArgv :: [Text],
    commandActions :: [CommandAction],
    commandSource :: CommandExecutionSource,
    commandStatus :: CommandStatus,
    commandOutput :: CommandExecutionOutput,
    commandDurationMs :: Maybe Millis
  }
  deriving (Eq, Show)

data CommandExecutionOutput = CommandExecutionOutput
  { commandAggregatedOutput :: Text,
    commandFormattedOutput :: Maybe Text,
    commandExitCode :: Maybe Int
  }
  deriving (Eq, Show)

data CommandAction
  = CommandRead PathText
  | CommandList PathText
  | CommandSearch Text
  | CommandUnknown Text
  deriving (Eq, Show)

data CommandExecutionSource
  = AgentShell
  | UserShell
  | UnifiedExecStartup
  | OtherCommandSource Text
  deriving (Eq, Show)

data CommandStatus
  = CommandRunning
  | CommandCompleted
  | CommandFailed
  deriving (Eq, Show)

-- Rust render family: PatchHistoryCell, plus related PlainHistoryCell failure notice.
-- Event provenance: ServerNotification::ItemStarted.item.FileChange.changes[];
-- ServerNotification::ItemCompleted.item.FileChange.status.
data FileChange = FileChange
  { fileChangePath :: PathText,
    fileChangeKind :: FileChangeKind,
    fileChangeDiff :: Maybe Text
  }
  deriving (Eq, Show)

data FileChangeKind
  = FileAdded
  | FileDeleted
  | FileModified
  deriving (Eq, Show)

data FileChangeStatus
  = FileChangeInProgress
  | FileChangeCompleted
  | FileChangeFailed
  deriving (Eq, Show)

-- Rust render families: McpToolCallCell, CompletedMcpToolCallWithImageOutput.
-- Event provenance: ServerNotification::ItemStarted.item.McpToolCall;
-- ServerNotification::ItemCompleted.item.McpToolCall.result; thread snapshot McpToolCall items.
data McpToolCallCell = McpToolCallCell
  { mcpItemId :: ItemId,
    mcpServer :: Text,
    mcpTool :: Text,
    mcpArgumentsJson :: Maybe Text,
    mcpResult :: Maybe McpResult
  }
  deriving (Eq, Show)

data McpResult
  = McpSuccess [McpContentBlock]
  | McpError Text
  | McpInterrupted
  deriving (Eq, Show)

data McpContentBlock
  = McpTextBlock Text
  | McpImageBlock
  | McpAudioBlock
  | McpResourceBlock UrlText
  | McpLinkBlock UrlText Text
  | McpUnknownBlock Text
  deriving (Eq, Show)

-- Rust render family: WebSearchCell.
-- Event provenance: ServerNotification::ItemStarted.item.WebSearch.id;
-- ServerNotification::ItemCompleted.item.WebSearch.query/action.
data WebSearchCell = WebSearchCell
  { webSearchItemId :: ItemId,
    webSearchQuery :: Text,
    webSearchAction :: WebSearchAction,
    webSearchLifecycle :: Lifecycle
  }
  deriving (Eq, Show)

data WebSearchAction
  = WebSearchSearch [Text]
  | WebSearchOpen UrlText
  | WebSearchFind Text UrlText
  | WebSearchOther
  deriving (Eq, Show)

-- Rust render family: HookCell.
-- Event provenance: ServerNotification::HookStarted.run; ServerNotification::HookCompleted.run.
data HookCell = HookCell
  { hookRuns :: [HookRun]
  }
  deriving (Eq, Show)

data HookRun = HookRun
  { hookRunId :: HookRunId,
    hookEventName :: Text,
    hookStatusMessage :: Maybe Text,
    hookStatus :: HookStatus,
    hookEntries :: [HookEntry]
  }
  deriving (Eq, Show)

data HookStatus
  = HookRunning
  | HookSucceeded
  | HookFailed
  | HookCancelled
  deriving (Eq, Show)

data HookEntry = HookEntry
  { hookEntryKind :: HookEntryKind,
    hookEntryText :: Text
  }
  deriving (Eq, Show)

data HookEntryKind
  = HookInfo
  | HookWarning
  | HookError
  | HookFeedback
  deriving (Eq, Show)

-- Rust render families: PlainHistoryCell, PrefixedWrappedHistoryCell, CyberPolicyNoticeCell,
-- DeprecationNoticeCell, FinalMessageSeparator, and approval/result notices.
-- Event provenance is carried by the 'NoticeKind' constructors below.
data NoticeCell = NoticeCell
  { noticeKind :: NoticeKind,
    noticeText :: Maybe Text
  }
  deriving (Eq, Show)

data NoticeKind
  = GenericErrorNotice
  | ServerOverloadedNotice
  | CyberPolicyNotice
  | WarningTextNotice
  | GuardianWarningTextNotice
  | ConfigWarningTextNotice
  | TrustedAccessForCyberNotice
  | DeprecationTextNotice (Maybe Text)
  | FileChangeFailedNotice ItemId
  | ImageViewNotice PathText
  | ImageGenerationDisplayNotice ImageGenerationNotice
  | AgentActivityNotice ItemId Text
  | ReviewModeEnteredNotice Text
  | ReviewModeExitedNotice
  | ContextCompactionNotice
  | GuardianApprovalResultNotice GuardianDecision GuardianAction
  | TurnSeparatorNotice (Maybe Millis)
  | TerminalInteractionNotice ItemId Text Text
  | NoteToSelfNotice Text
  deriving (Eq, Show)

data ImageGenerationNotice = ImageGenerationNotice
  { imageGenerationId :: ItemId,
    imageGenerationStatus :: Text,
    imageGenerationPrompt :: Maybe Text,
    imageGenerationSavedPath :: Maybe PathText
  }
  deriving (Eq, Show)

data GuardianDecision
  = GuardianApproved
  | GuardianDenied
  | GuardianTimedOut
  | GuardianAborted
  deriving (Eq, Show)

data GuardianAction
  = GuardianCommandAction [Text]
  | GuardianPatchAction
  | GuardianMcpAction Text Text
  | GuardianNetworkAction Text
  | GuardianPermissionAction
  | GuardianOtherAction Text
  deriving (Eq, Show)

data Lifecycle
  = Running
  | Completed
  deriving (Eq, Show)

-- Rust render family: RequestUserInputResultCell.
-- Event provenance: ServerRequest::ToolRequestUserInput.params.questions plus a later
-- ClientSubmittedUserInput or ClientInterruptedRequest event.
data RequestUserInputResultCell = RequestUserInputResultCell
  { requestUserInputQuestions :: [UserInputQuestion],
    requestUserInputAnswers :: [UserInputAnswer],
    requestUserInputInterrupted :: Bool
  }
  deriving (Eq, Show)

data PendingUserInput = PendingUserInput
  { pendingUserInputCallId :: ItemId,
    pendingUserInputQuestions :: [UserInputQuestion]
  }
  deriving (Eq, Show)

data UserInputQuestion = UserInputQuestion
  { userInputQuestionId :: Text,
    userInputQuestionText :: Text,
    userInputQuestionSecret :: Bool,
    userInputQuestionOptions :: [UserInputOption]
  }
  deriving (Eq, Show)

data UserInputOption = UserInputOption
  { userInputOptionLabel :: Text,
    userInputOptionValue :: Text
  }
  deriving (Eq, Show)

data UserInputAnswer = UserInputAnswer
  { userInputAnswerQuestionId :: Text,
    userInputAnswerValues :: [Text],
    userInputAnswerNote :: Maybe Text
  }
  deriving (Eq, Show)

data PendingApproval = PendingApproval
  { pendingApprovalKind :: ApprovalKind,
    pendingApprovalItemId :: Maybe ItemId,
    pendingApprovalSummary :: Text
  }
  deriving (Eq, Show)

data ApprovalKind
  = CommandApproval
  | FileChangeApproval
  | PermissionsApproval
  deriving (Eq, Show)

data ReducerDiagnostic = ReducerDiagnostic
  { diagnosticReason :: DiagnosticReason,
    diagnosticItemId :: Maybe ItemId,
    diagnosticMessage :: Text
  }
  deriving (Eq, Show)

data DiagnosticReason
  = OrphanDelta
  | OrphanCompletion
  | UnsupportedInput
  deriving (Eq, Show)

reduce :: TranscriptState -> TranscriptInput -> TranscriptState
reduce state input =
  case input of
    ThreadSnapshotInput snapshot -> reduceThreadSnapshot state snapshot
    ServerNotificationInput notification -> reduceNotification state notification
    ServerRequestInput request -> reduceServerRequest state request
    ClientInteractionInput interaction -> reduceClientInteraction state interaction

reduceThreadSnapshot :: TranscriptState -> ThreadSnapshot -> TranscriptState
reduceThreadSnapshot state snapshot =
  foldl' reduceSnapshotTurn state (snapshotTurns snapshot)

reduceSnapshotTurn :: TranscriptState -> Turn -> TranscriptState
reduceSnapshotTurn state turn =
  appendTurnSeparator $
    foldl' appendCompletedThreadItem state (turnItems turn)
  where
    appendTurnSeparator current =
      case turnStatus turn of
        TurnCompleted -> appendCell current (NoticeDisplay (NoticeCell (TurnSeparatorNotice (turnDurationMs turn)) Nothing))
        TurnFailed -> appendCell current (NoticeDisplay (NoticeCell (TurnSeparatorNotice (turnDurationMs turn)) Nothing))
        TurnInterrupted -> markInterruptedActiveCells current
        TurnInProgress -> current

reduceNotification :: TranscriptState -> ServerNotification -> TranscriptState
reduceNotification state notification =
  case notification of
    ThreadStarted snapshot -> reduceThreadSnapshot state snapshot
    TurnStarted _threadId turn -> foldl' appendCompletedThreadItem state (turnItems turn)
    TurnCompletedNotification _threadId turn -> reduceTurnCompleted state turn
    ItemStarted _threadId _turnId _startedAt item -> startThreadItem state item
    ItemCompleted _threadId _turnId _completedAt item -> completeThreadItem state item
    AgentMessageDelta _threadId _turnId itemId delta -> appendAssistantDelta state itemId delta
    PlanDelta _threadId _turnId itemId delta -> appendPlanDelta state itemId delta
    ReasoningSummaryTextDelta _threadId _turnId itemId _summaryIndex delta ->
      appendReasoningSummaryDelta state itemId delta
    ReasoningSummaryPartAdded _threadId _turnId itemId _summaryIndex ->
      appendReasoningSummaryDelta state itemId "\n\n"
    ReasoningTextDelta _threadId _turnId itemId _contentIndex delta ->
      appendReasoningRawDelta state itemId delta
    CommandExecutionOutputDelta _threadId _turnId itemId delta ->
      appendCommandOutputDelta state itemId delta
    TerminalInteraction _threadId _turnId itemId processId stdin ->
      appendCell state (NoticeDisplay (NoticeCell (TerminalInteractionNotice itemId processId stdin) Nothing))
    TurnPlanUpdated _threadId _turnId explanation plan ->
      appendCell state (PlanUpdateDisplay explanation plan)
    HookStarted run -> startHookRun state run
    HookCompleted run -> completeHookRun state run
    WarningNotice message ->
      appendCell state (NoticeDisplay (NoticeCell WarningTextNotice (Just message)))
    GuardianWarningNotice message ->
      appendCell state (NoticeDisplay (NoticeCell GuardianWarningTextNotice (Just message)))
    ConfigWarningNotice summary details ->
      appendCell state (NoticeDisplay (NoticeCell ConfigWarningTextNotice (Just (joinConfigWarning summary details))))
    ModelVerificationNotice verifications ->
      if TrustedAccessForCyber `elem` verifications
        then appendCell state (NoticeDisplay (NoticeCell TrustedAccessForCyberNotice Nothing))
        else state
    ErrorNotice err -> reduceTurnError state err
    DeprecationNotice summary details ->
      appendCell state (NoticeDisplay (NoticeCell (DeprecationTextNotice details) (Just summary)))
    GuardianApprovalReviewCompleted _threadId _turnId decision action ->
      appendCell state (NoticeDisplay (NoticeCell (GuardianApprovalResultNotice decision action) Nothing))
    ServerRequestResolved requestId ->
      removePendingRequest requestId state
    IgnoredNotification label ->
      addDiagnostic UnsupportedInput Nothing ("ignored notification: " <> label) state

reduceTurnCompleted :: TranscriptState -> Turn -> TranscriptState
reduceTurnCompleted state turn =
  case turnStatus turn of
    TurnInterrupted -> markInterruptedActiveCells state
    TurnCompleted ->
      appendCell state (NoticeDisplay (NoticeCell (TurnSeparatorNotice (turnDurationMs turn)) Nothing))
    TurnFailed ->
      appendCell state (NoticeDisplay (NoticeCell (TurnSeparatorNotice (turnDurationMs turn)) Nothing))
    TurnInProgress -> state

reduceServerRequest :: TranscriptState -> ServerRequest -> TranscriptState
reduceServerRequest state request =
  case request of
    CommandExecutionRequestApproval requestId approval ->
      insertPendingApproval requestId approval state
    FileChangeRequestApproval requestId approval ->
      insertPendingApproval requestId approval state
    PermissionsRequestApproval requestId approval ->
      insertPendingApproval requestId approval state
    McpServerElicitationRequest requestId label ->
      state {pendingInteractions = (pendingInteractions state) {pendingMcpElicitations = Map.insert requestId label (pendingMcpElicitations (pendingInteractions state))}}
    ToolRequestUserInput requestId prompt ->
      state {pendingInteractions = (pendingInteractions state) {pendingUserInputs = Map.insert requestId prompt (pendingUserInputs (pendingInteractions state))}}
    UnsupportedServerRequest requestId label ->
      addDiagnostic UnsupportedInput Nothing ("unsupported server request " <> label <> " with id " <> requestIdText requestId) state

reduceClientInteraction :: TranscriptState -> ClientInteraction -> TranscriptState
reduceClientInteraction state interaction =
  case interaction of
    ClientSubmittedUserInput requestId answers ->
      case Map.lookup requestId (pendingUserInputs (pendingInteractions state)) of
        Nothing -> addDiagnostic OrphanCompletion Nothing "submitted user input for unknown request" state
        Just pending ->
          removePendingRequest requestId $
            appendCell state $
              RequestUserInputResultDisplay
                RequestUserInputResultCell
                  { requestUserInputQuestions = pendingUserInputQuestions pending,
                    requestUserInputAnswers = answers,
                    requestUserInputInterrupted = False
                  }
    ClientInterruptedRequest requestId ->
      case Map.lookup requestId (pendingUserInputs (pendingInteractions state)) of
        Nothing -> removePendingRequest requestId state
        Just pending ->
          removePendingRequest requestId $
            appendCell state $
              RequestUserInputResultDisplay
                RequestUserInputResultCell
                  { requestUserInputQuestions = pendingUserInputQuestions pending,
                    requestUserInputAnswers = [],
                    requestUserInputInterrupted = True
                  }
    ClientResolvedApproval requestId decision ->
      removePendingRequest requestId $
        appendCell state $
          NoticeDisplay (NoticeCell (GuardianApprovalResultNotice decision (GuardianOtherAction "client approval")) Nothing)

startThreadItem :: TranscriptState -> ThreadItem -> TranscriptState
startThreadItem state item =
  case item of
    CommandExecutionItem command ->
      state {activeCells = (activeCells state) {activeCommands = Map.insert (commandItemId command) command (activeCommands (activeCells state))}}
    McpToolCallItem call ->
      state {activeCells = (activeCells state) {activeMcpCalls = Map.insert (mcpItemId call) call (activeMcpCalls (activeCells state))}}
    WebSearchItem search ->
      state {activeCells = (activeCells state) {activeWebSearches = Map.insert (webSearchItemId search) (search {webSearchLifecycle = Running}) (activeWebSearches (activeCells state))}}
    FileChangeItem itemId status changes ->
      appendCell state (PatchDisplay itemId status changes)
    CollabAgentToolCallItem itemId text ->
      appendCell state (NoticeDisplay (NoticeCell (AgentActivityNotice itemId text) Nothing))
    SubAgentActivityItem itemId text ->
      appendCell state (NoticeDisplay (NoticeCell (AgentActivityNotice itemId text) Nothing))
    EnteredReviewModeItem _itemId review ->
      appendCell state (NoticeDisplay (NoticeCell (ReviewModeEnteredNotice review) Nothing))
    ExitedReviewModeItem _itemId ->
      appendCell state (NoticeDisplay (NoticeCell ReviewModeExitedNotice Nothing))
    _ -> state

completeThreadItem :: TranscriptState -> ThreadItem -> TranscriptState
completeThreadItem state item =
  case item of
    AgentMessageItem itemId text ->
      completeAssistantMessage state itemId text
    PlanItem itemId text ->
      completePlan state itemId text
    ReasoningItem itemId summary raw ->
      completeReasoning state itemId summary raw
    CommandExecutionItem command ->
      completeCommand state command
    McpToolCallItem call ->
      completeMcpCall state call
    WebSearchItem search ->
      completeWebSearch state search
    FileChangeItem itemId FileChangeFailed _changes ->
      appendCell state (NoticeDisplay (NoticeCell (FileChangeFailedNotice itemId) Nothing))
    _ -> appendCompletedThreadItem state item

appendCompletedThreadItem :: TranscriptState -> ThreadItem -> TranscriptState
appendCompletedThreadItem state item =
  foldl' appendCell state (displayCellsFromThreadItem item)

displayCellsFromThreadItem :: ThreadItem -> [DisplayCell]
displayCellsFromThreadItem item =
  case item of
    UserMessageItem _itemId inputs -> [UserMessageDisplay (userMessageFromInputs inputs)]
    AgentMessageItem _itemId markdown -> [AssistantMessageDisplay (AssistantMessageCell markdown)]
    ReasoningItem _itemId summary raw -> [ReasoningDisplay (ReasoningCell (Text.intercalate "\n\n" summary) raw False)]
    PlanItem itemId markdown -> [PlanDisplay (PlanCell itemId markdown Completed)]
    CommandExecutionItem command -> [CommandExecutionDisplay command]
    FileChangeItem itemId status changes -> [PatchDisplay itemId status changes]
    McpToolCallItem call -> McpToolCallDisplay call : imageMarkerCells call
    WebSearchItem search -> [WebSearchDisplay (search {webSearchLifecycle = Completed})]
    NoteToSelfItem _itemId note -> [NoticeDisplay (NoticeCell (NoteToSelfNotice note) Nothing)]
    ImageViewItem _itemId path -> [NoticeDisplay (NoticeCell (ImageViewNotice path) Nothing)]
    ImageGenerationItem _itemId notice -> [NoticeDisplay (NoticeCell (ImageGenerationDisplayNotice notice) Nothing)]
    CollabAgentToolCallItem itemId text -> [NoticeDisplay (NoticeCell (AgentActivityNotice itemId text) Nothing)]
    SubAgentActivityItem itemId text -> [NoticeDisplay (NoticeCell (AgentActivityNotice itemId text) Nothing)]
    EnteredReviewModeItem _itemId review -> [NoticeDisplay (NoticeCell (ReviewModeEnteredNotice review) Nothing)]
    ExitedReviewModeItem _itemId -> [NoticeDisplay (NoticeCell ReviewModeExitedNotice Nothing)]
    ContextCompactionItem _itemId -> [NoticeDisplay (NoticeCell ContextCompactionNotice Nothing)]
    UnknownThreadItem _itemId _label -> []

appendAssistantDelta :: TranscriptState -> ItemId -> Text -> TranscriptState
appendAssistantDelta state itemId delta =
  let active = activeCells state
      draft =
        Map.findWithDefault
          (AssistantMessageDraft itemId "")
          itemId
          (activeAssistantMessages active)
      updated = draft {assistantDraftMarkdown = assistantDraftMarkdown draft <> delta}
   in state {activeCells = active {activeAssistantMessages = Map.insert itemId updated (activeAssistantMessages active)}}

appendPlanDelta :: TranscriptState -> ItemId -> Text -> TranscriptState
appendPlanDelta state itemId delta =
  let active = activeCells state
      current =
        Map.findWithDefault
          (PlanCell itemId "" Running)
          itemId
          (activePlans active)
      updated = current {planMarkdown = planMarkdown current <> delta, planLifecycle = Running}
   in state {activeCells = active {activePlans = Map.insert itemId updated (activePlans active)}}

appendReasoningSummaryDelta :: TranscriptState -> ItemId -> Text -> TranscriptState
appendReasoningSummaryDelta state itemId delta =
  let active = activeCells state
      draft =
        Map.findWithDefault
          (ReasoningDraft itemId [] [])
          itemId
          (activeReasoning active)
      updated = draft {reasoningDraftSummaryParts = reasoningDraftSummaryParts draft <> [delta]}
   in state {activeCells = active {activeReasoning = Map.insert itemId updated (activeReasoning active)}}

appendReasoningRawDelta :: TranscriptState -> ItemId -> Text -> TranscriptState
appendReasoningRawDelta state itemId delta =
  let active = activeCells state
      draft =
        Map.findWithDefault
          (ReasoningDraft itemId [] [])
          itemId
          (activeReasoning active)
      updated = draft {reasoningDraftRawParts = reasoningDraftRawParts draft <> [delta]}
   in state {activeCells = active {activeReasoning = Map.insert itemId updated (activeReasoning active)}}

appendCommandOutputDelta :: TranscriptState -> ItemId -> Text -> TranscriptState
appendCommandOutputDelta state itemId delta =
  let active = activeCells state
   in case Map.lookup itemId (activeCommands active) of
        Nothing -> addDiagnostic OrphanDelta (Just itemId) "command output delta without active command" state
        Just command ->
          let output = commandOutput command
              updatedOutput = output {commandAggregatedOutput = commandAggregatedOutput output <> delta}
              updatedCommand = command {commandOutput = updatedOutput}
           in state {activeCells = active {activeCommands = Map.insert itemId updatedCommand (activeCommands active)}}

completeAssistantMessage :: TranscriptState -> ItemId -> Text -> TranscriptState
completeAssistantMessage state itemId completedText =
  let active = activeCells state
      markdown =
        case Map.lookup itemId (activeAssistantMessages active) of
          Just draft -> assistantDraftMarkdown draft
          Nothing -> completedText
      nextActive = active {activeAssistantMessages = Map.delete itemId (activeAssistantMessages active)}
   in appendCell
        (state {activeCells = nextActive})
        (AssistantMessageDisplay (AssistantMessageCell markdown))

completePlan :: TranscriptState -> ItemId -> Text -> TranscriptState
completePlan state itemId completedText =
  let active = activeCells state
      markdown =
        maybe completedText planMarkdown (Map.lookup itemId (activePlans active))
      nextActive = active {activePlans = Map.delete itemId (activePlans active)}
   in appendCell
        (state {activeCells = nextActive})
        (PlanDisplay (PlanCell itemId markdown Completed))

completeReasoning :: TranscriptState -> ItemId -> [Text] -> [Text] -> TranscriptState
completeReasoning state itemId completedSummary completedRaw =
  let active = activeCells state
      (summary, raw) =
        case Map.lookup itemId (activeReasoning active) of
          Just draft -> (reasoningDraftSummaryParts draft, reasoningDraftRawParts draft)
          Nothing -> (completedSummary, completedRaw)
      nextActive = active {activeReasoning = Map.delete itemId (activeReasoning active)}
   in appendCell
        (state {activeCells = nextActive})
        (ReasoningDisplay (ReasoningCell (Text.intercalate "\n\n" summary) raw False))

completeCommand :: TranscriptState -> CommandExecutionCell -> TranscriptState
completeCommand state completedCommand =
  let itemId = commandItemId completedCommand
      active = activeCells state
      command =
        case Map.lookup itemId (activeCommands active) of
          Just running -> mergeCommandCompletion running completedCommand
          Nothing -> completedCommand
      nextActive = active {activeCommands = Map.delete itemId (activeCommands active)}
      nextState =
        if Map.member itemId (activeCommands active)
          then state {activeCells = nextActive}
          else addDiagnostic OrphanCompletion (Just itemId) "command completion without active command" (state {activeCells = nextActive})
   in appendCell nextState (CommandExecutionDisplay command)

completeMcpCall :: TranscriptState -> McpToolCallCell -> TranscriptState
completeMcpCall state completedCall =
  let itemId = mcpItemId completedCall
      active = activeCells state
      call =
        maybe completedCall (`mergeMcpCompletion` completedCall) (Map.lookup itemId (activeMcpCalls active))
      nextActive = active {activeMcpCalls = Map.delete itemId (activeMcpCalls active)}
      nextState =
        if Map.member itemId (activeMcpCalls active)
          then state {activeCells = nextActive}
          else addDiagnostic OrphanCompletion (Just itemId) "MCP completion without active call" (state {activeCells = nextActive})
   in foldl' appendCell nextState (McpToolCallDisplay call : imageMarkerCells call)

completeWebSearch :: TranscriptState -> WebSearchCell -> TranscriptState
completeWebSearch state completedSearch =
  let itemId = webSearchItemId completedSearch
      active = activeCells state
      search =
        maybe completedSearch (`mergeWebSearchCompletion` completedSearch) (Map.lookup itemId (activeWebSearches active))
      nextActive = active {activeWebSearches = Map.delete itemId (activeWebSearches active)}
      nextState =
        if Map.member itemId (activeWebSearches active)
          then state {activeCells = nextActive}
          else addDiagnostic OrphanCompletion (Just itemId) "web search completion without active search" (state {activeCells = nextActive})
   in appendCell nextState (WebSearchDisplay (search {webSearchLifecycle = Completed}))

startHookRun :: TranscriptState -> HookRun -> TranscriptState
startHookRun state run =
  let active = activeCells state
      cell = HookCell [run {hookStatus = HookRunning}]
   in state {activeCells = active {activeHooks = Map.insert (hookRunId run) cell (activeHooks active)}}

completeHookRun :: TranscriptState -> HookRun -> TranscriptState
completeHookRun state run =
  let active = activeCells state
      nextActive = active {activeHooks = Map.delete (hookRunId run) (activeHooks active)}
      nextState =
        if Map.member (hookRunId run) (activeHooks active)
          then state {activeCells = nextActive}
          else addDiagnostic OrphanCompletion Nothing "hook completion without active run" (state {activeCells = nextActive})
   in appendCell nextState (HookDisplay (HookCell [run]))

markInterruptedActiveCells :: TranscriptState -> TranscriptState
markInterruptedActiveCells state =
  let active = activeCells state
      interruptedMcp =
        fmap
          (\call -> call {mcpResult = Just McpInterrupted})
          (activeMcpCalls active)
      stateWithMcp =
        foldl'
          appendCell
          (state {activeCells = active {activeMcpCalls = Map.empty}})
          (McpToolCallDisplay <$> Map.elems interruptedMcp)
   in stateWithMcp

reduceTurnError :: TranscriptState -> TurnError -> TranscriptState
reduceTurnError state err
  | errorRetrying err = state
  | otherwise =
      case errorInfo err of
        Just CyberPolicy ->
          appendCell state (NoticeDisplay (NoticeCell CyberPolicyNotice Nothing))
        Just ServerOverloaded ->
          appendCell state (NoticeDisplay (NoticeCell ServerOverloadedNotice (Just (serverOverloadedText err))))
        _ ->
          appendCell state (NoticeDisplay (NoticeCell GenericErrorNotice (Just (errorMessage err))))

removePendingRequest :: RequestId -> TranscriptState -> TranscriptState
removePendingRequest requestId state =
  let pending = pendingInteractions state
   in state
        { pendingInteractions =
            pending
              { pendingUserInputs = Map.delete requestId (pendingUserInputs pending),
                pendingApprovals = Map.delete requestId (pendingApprovals pending),
                pendingMcpElicitations = Map.delete requestId (pendingMcpElicitations pending)
              }
        }

insertPendingApproval :: RequestId -> PendingApproval -> TranscriptState -> TranscriptState
insertPendingApproval requestId approval state =
  let pending = pendingInteractions state
   in state {pendingInteractions = pending {pendingApprovals = Map.insert requestId approval (pendingApprovals pending)}}

appendCell :: TranscriptState -> DisplayCell -> TranscriptState
appendCell state cell =
  state {transcriptCells = transcriptCells state |> cell}

addDiagnostic :: DiagnosticReason -> Maybe ItemId -> Text -> TranscriptState -> TranscriptState
addDiagnostic reason itemId message state =
  state
    { diagnostics =
        diagnostics state
          |> ReducerDiagnostic
            { diagnosticReason = reason,
              diagnosticItemId = itemId,
              diagnosticMessage = message
            }
    }

userMessageFromInputs :: [UserInput] -> UserMessageCell
userMessageFromInputs inputs =
  UserMessageCell
    { userMessageText = Text.concat [text | TextInput text _ <- inputs],
      userTextElements = concat [elements | TextInput _ elements <- inputs],
      userRemoteImageUrls = [url | RemoteImageInput url <- inputs]
    }

mergeCommandCompletion :: CommandExecutionCell -> CommandExecutionCell -> CommandExecutionCell
mergeCommandCompletion running completed =
  completed
    { commandOutput =
        let completedOutput = commandOutput completed
         in completedOutput
              { commandAggregatedOutput =
                  if Text.null (commandAggregatedOutput completedOutput)
                    then commandAggregatedOutput (commandOutput running)
                    else commandAggregatedOutput completedOutput
              }
    }

mergeMcpCompletion :: McpToolCallCell -> McpToolCallCell -> McpToolCallCell
mergeMcpCompletion running completed =
  completed
    { mcpArgumentsJson = firstJust (mcpArgumentsJson completed) (mcpArgumentsJson running)
    }

mergeWebSearchCompletion :: WebSearchCell -> WebSearchCell -> WebSearchCell
mergeWebSearchCompletion running completed =
  completed
    { webSearchQuery =
        if Text.null (webSearchQuery completed)
          then webSearchQuery running
          else webSearchQuery completed
    }

imageMarkerCells :: McpToolCallCell -> [DisplayCell]
imageMarkerCells call =
  case mcpResult call of
    Just (McpSuccess blocks)
      | any isImageBlock blocks -> [McpImageOutputMarkerDisplay (mcpItemId call)]
    _ -> []

isImageBlock :: McpContentBlock -> Bool
isImageBlock block =
  case block of
    McpImageBlock -> True
    _ -> False

joinConfigWarning :: Text -> Maybe Text -> Text
joinConfigWarning summary details =
  case details of
    Nothing -> summary
    Just extra -> summary <> ": " <> extra

serverOverloadedText :: TurnError -> Text
serverOverloadedText err
  | Text.null (errorMessage err) = "Codex is currently experiencing high load."
  | otherwise = errorMessage err

requestIdText :: RequestId -> Text
requestIdText (RequestId text) = text

firstJust :: Maybe a -> Maybe a -> Maybe a
firstJust left right =
  case left of
    Just value -> Just value
    Nothing -> right
