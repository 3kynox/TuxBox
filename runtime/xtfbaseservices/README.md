# XTF base services

The provider implements the process-wide assertion-enable state API: `XtfErrAreAssertsEnabled`, `XtfErrPushAssertsEnabled` and `XtfErrPopAssertsEnabled`. Push saves the current state and pop restores it; an empty pop leaves the state unchanged. The checked control compares the original and this provider, including nested changes and a second thread.

`XtfLogGetDebugType` and `XtfLogGetTextColors` return severity labels and Windows console color attributes; unknown levels return null and zero. The paired control checks every defined level and out-of-range values.

Assertion handlers, assertion reporting and the rest of XTF base services are not implemented. The provider is not staged for games by default.
