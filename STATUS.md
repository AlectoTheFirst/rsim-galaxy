# Status

| Crate | Content | State |
|---|---|---|
| `galaxy-scene` | The relations, the rule set, the world, the style, the operations with their log, the wire form and the reader of the relations format | In progress |
| `galaxy-render` | The draw code on `wgpu` and the WGSL shaders | In progress |
| `galaxy-native` | The viewport: the desktop window, the MCP server, the sources with the load and the render with no window | In progress |
| `galaxy-web` | The WebAssembly build for a canvas | Not started |

The Cargo workspace has 3 crates, `galaxy-scene`, `galaxy-render` and `galaxy-native`.

## galaxy-scene

Complete:

- the relations: nodes with a kind, a parent, a label and the data of the source, and flows with their path, their outcome and the data of the source
- the rule set with the arrangements row, stack, grid and layers, the order of each column of the arrangement layers that reduces the crossings of the flows, and the shapes box, cylinder, router, switch, firewall, device, access-point, workstation, cloud, layer3-switch, server, laptop and load-balancer, and the expansion that gives each node a box
- the library: the operation define_shape with the parts box, cylinder and prism in the unit box of the shape and the symbol, the operation set_material with the built-in materials and up to 64 materials, up to 64 defined shapes with a vertex budget of 1,000,000 for the library, and the rules that name a defined shape
- the flow lines through the doors and the corridors of the containers of their path, with the corridor of a stack beside its children, the line between 2 adjacent containers of a row through their facing sides, and the wires of the arrangement layers on the floor through the gaps and the free stretches of the columns
- the lanes of the flow lines: the segments of a bundle side by side in distinct lanes, in the order of the sides from which they come and of the lanes of the runs before and after them, with a spacing for each bundle that keeps clear of parallel segments, the lanes at the side door of a router, a switch or an access point outside the node, the lanes along x in the corridor of a stack and along y at the doors of its children, the lane change at the corner of a run along y, 1 lane change on a segment along y between 2 runs, no common stretch of 2 lines outside a node, and the lanes of the last drawn paths kept in the world
- the check of a world for a child outside its parent, for siblings that intersect and for a flow line through a node off its path
- the style: the rule set and the look
- the operations add node, add flow, set rule, set look, define shape, set material and clear, with the refusals, the log, undo and replay
- the label of a node from `add_node` and from `node.label` of a load, the id as the label of a node with no label, `label` of `set_rule` for each kind, and the label in the world text
- the load: the reader of a relations document and of a stream of joined pages with the checks, the limits and the versions of the format, the framing of the pages with the place of each line, the first 20 refusals in the order of the document, the members of a document in the order of the text, the answer with the source, the counts, the kinds, the unit, partial, ignored by the name rule up to 64 names and not_shown, the load as 1 entry of the log with undo of the whole load, the wire form `load_view` with the `sha256` of the bytes, and SHA-256
- the wire form with a JSON schema for each operation, and the refusal of a JSON object with a member name 2 times, of a `\u` escape with no 4 hex digits, of a field name outside the name rule, of more than 64 fields and of a text value of a field outside 1 to 1024 characters
- the scene of the primitives globe, building, arc and label with its operations and its log
- the flow data in the relations: the magnitude with its unit, the window of a load, the group, the reply and the hops of a flow, with the refusals of `add_flow`, the reader, the world text and the largest magnitude of a world
- the look values widths (magnitude, uniform) and scale (log, linear)
- the built-in flow materials flow-1, flow-2, flow-drop, flow-loop, flow-unknown and flow-reply

Open:

- the arrangements ring and shell on a globe
- the scene of the primitives and the world as 1 model

## galaxy-render

Complete:

- the frame of a world into a window or an image, and its PNG file
- the virtual detail: the detail state of each node from its size on the screen, the culling of the nodes and of the marks of the flow lines outside the view, the lanes of the drawn paths, and the trunks of the flows below 2 pixels of spacing
- 1 thickness for each flow line from its magnitude on the log or the linear scale, or uniform, the sum of the magnitudes of a trunk, 2 pixels or more on each piece of a segment, and no line inside a node of its path that is not open
- the flow lines in their lanes with a casing and no casing on a lane change, the start dot, the arrowheads in the direction of travel every 160 pixels and on each leg between 2 nodes, clear of crossings and bends, the colour of the flow materials with the colours of set_material, the colour of a group, the stop mark of a dropped flow, the halo of a selected flow and the casing of the other branches of its group
- the dashes of a reply from its start dot, with the casing through the gaps
- the packets on virtual time from the host program, and the selection of a trunk
- the home view that fills the frame
- the looks: perspective and isometric, ground and floating, sun and flat light, crisp and glow lines, light and dark scheme
- the floor grid and the compass
- the mesh registry with the meshes of the shapes box, cylinder, router, switch, firewall, device, access-point, workstation, cloud, layer3-switch, server, laptop and load-balancer, and of the arrowheads, and the material slots body, symbol, brick, flame, mortar, flame core and the 6 flow materials with a colour for each scheme
- the symbol of the router, the switch, the layer-3 switch, the access point and the load balancer on its top and on its front, the top symbol hidden below 4 pixels and the front symbol below a stroke of 1.8 pixels, by 1 rule in a stack and alone
- the symbols of the router, the switch, the layer-3 switch, the access point and the load balancer, the flame of the firewall and the symbols of the defined shapes in the aspect of their reference size at each node size, each filling its face on the shorter side
- the stadium of the router and the round corners of the access point at each node size
- the upright cloud at the back of the floor of an open node of the cloud shape
- no top symbol on a node below another node of a stack, with its body up to the node above it, and the brick block of a firewall in that place
- the edges of the open nodes behind the nodes and the lines in them
- the meshes of the defined shapes from their parts, built again when the library changes, and the material table with the colours of set_material for up to 64 materials, with the colour of the body
- the shading with sky light, edge bands, contact shade and highlight, and the floor shadows
- the panel of the selection with the data of the source, in the freest corner of the frame, in Inter and JetBrains Mono from the distance-field atlas
- the labels of the nodes: the place at each detail state, JetBrains Mono at 600 with a halo, the selection colour of the selected node, the priority order, the greedy placement with the overlap grid, no label cut by the frame, the limits of the candidates and the glyphs, and the scene text of the shown and the hidden labels
- the signed distance-field atlas of the glyphs from the coverage of `ab_glyph` by 8SSEDT, cleared when it is full
- the scale factor of the window for the labels, the panel and the compass
- the pick of the object at a pixel, and the edges of the selected object in the selection colour

Open:

- the hatch beside a panel value with no information
- the blend between 2 detail states
- cast shadows and ambient occlusion
- the import of meshes through the crate `gltf`

## galaxy-native

Complete:

- the desktop window with the camera and the selection, with the panel of the selected object and its line `label`, and the scale factor of the window for the labels, the panel and the compass
- the MCP server with the tools of the wire form and the tools undo, describe, frame and sources
- the operation file, the frame with no window with `--time` and the operation log as an operation file
- the panel lines of a flow with its magnitude, the window, its group, its request, its replies and its hops, and of a trunk with its count and its sum
- the time line of the window with play, pause, step, speed and position on virtual time
- the label lines of a frame in the text of the tool `frame` and on the standard error of `--png`
- the configuration of the sources with its checks and the credential references `env:` and `file:`
- `load_view` over the MCP transport with pages and from a file, the answer to `ping` and the refusal of each other request of the source, the limits of the pages, the lines and the cursor, the limits of a `.json` and a `.jsonl` file before it is read, a `.json` file as a document and a `.jsonl` file as a stream, the command of a source in the directory of the configuration, the kept bytes in `galaxy-documents` as sent, the replay of a load from the kept bytes for a source of the configuration, and the refusals of the arguments, no_source, source, credential and line
- the check of the credential in the bytes, in the decoded JSON strings and in the shown texts of a document, and in the whole text of a refusal of the source

Open:

- the selection as an event to the agent
- the HTTP transport of `load_view`
