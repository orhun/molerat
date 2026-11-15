# Accessibility

Molerat helps terminal applications expose their interface to people using assistive technology
and to software agents. It provides a semantic API with AT-SPI and MCP integrations.

## Priorities

- Expose meaningful labels, roles, values, states, focus and structure.
- Make keyboard and mouse actions available through the semantic interface.
- Keep semantic output predictable and practical for application authors to maintain.

## Contributor expectations

Keep the semantic tree synchronized with the visible interface and application state. User-facing
changes should include clear labels, relevant state, focus, structure, and actions. Test AT-SPI
changes with Orca or Accerciser when possible, and test MCP changes through `get_ui` and the
affected actions.

## Reporting accessibility issues

Open a [bug report](https://github.com/orhun/molerat/issues/new?template=bug_report.yml) describing
the affected task, observed behavior, environment, and assistive technology. Screenshots,
recordings, and disability disclosure are not required. If GitHub is a barrier, email
[orhunparmaksiz@gmail.com](mailto:orhunparmaksiz@gmail.com).

## Supported environments

- **AT-SPI:** Linux desktop environments with a working AT-SPI bus.
- **MCP:** clients that can launch the `molerat-mcp` standard-input/output bridge.

## Known limitations

Molerat does not infer semantics from terminal cells. Application authors must keep the semantic
tree aligned with the visible interface, so support varies between applications. Native
screen-reader support currently relies on Linux AT-SPI; Windows and macOS adapters are not yet
provided. The project has not undergone an independent accessibility audit.
