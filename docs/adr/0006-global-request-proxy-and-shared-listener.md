# Share one listener with the global Request Proxy

The global Request Proxy shares one listener with the Console, giving clients
one endpoint without tying traffic to Tenants, Runs, Sessions, or Agents.
Management routes require loopback access even when the listener accepts remote
proxy traffic.
