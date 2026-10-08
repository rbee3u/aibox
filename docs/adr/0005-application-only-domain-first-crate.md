# Keep AIBox application-only

AIBox exposes only its application entry point; management belongs to the
foreground Service and embedded Console. Domain facades and the Control API
remain internal, avoiding a public embedding contract for application workflows.
