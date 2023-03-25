use crate::{
    camera::PickingOptions,
    interaction::{
        calculate_gizmo_distance_scaling,
        gizmo::move_gizmo::MoveGizmo,
        navmesh::{data_model::NavmeshEntity, selection::NavmeshSelection},
        plane::PlaneKind,
        InteractionMode,
    },
    scene::{
        commands::{
            navmesh::{
                AddNavmeshEdgeCommand, ConnectNavmeshEdgesCommand, DeleteNavmeshVertexCommand,
                MoveNavmeshVertexCommand,
            },
            ChangeSelectionCommand, CommandGroup, SceneCommand,
        },
        EditorScene, Selection,
    },
    settings::Settings,
    utils::window_content,
    GameEngine, Message, Mode,
};
use fyrox::core::math::TriangleEdge;
use fyrox::scene::navmesh::NavigationalMesh;
use fyrox::utils::astar::PathVertex;
use fyrox::{
    core::{
        algebra::{Vector2, Vector3},
        color::Color,
        math::ray::CylinderKind,
        pool::Handle,
        scope_profile,
    },
    gui::{
        button::{ButtonBuilder, ButtonMessage},
        grid::{Column, GridBuilder, Row},
        message::{KeyCode, MessageDirection, UiMessage},
        stack_panel::StackPanelBuilder,
        widget::{WidgetBuilder, WidgetMessage},
        window::{WindowBuilder, WindowTitle},
        BuildContext, Orientation, Thickness, UiNode, UserInterface,
    },
    scene::{camera::Camera, node::Node},
};
use std::{collections::HashMap, sync::mpsc::Sender};

pub mod data_model;
pub mod selection;

pub struct NavmeshPanel {
    pub window: Handle<UiNode>,
    connect: Handle<UiNode>,
    sender: Sender<Message>,
    selected: Handle<Node>,
}

impl NavmeshPanel {
    pub fn new(ctx: &mut BuildContext, sender: Sender<Message>) -> Self {
        let connect;
        let window = WindowBuilder::new(WidgetBuilder::new())
            .with_title(WindowTitle::text("Navmesh"))
            .with_content(
                GridBuilder::new(
                    WidgetBuilder::new().with_child(
                        StackPanelBuilder::new(WidgetBuilder::new().with_child({
                            connect = ButtonBuilder::new(
                                WidgetBuilder::new().with_margin(Thickness::uniform(1.0)),
                            )
                            .with_text("Connect")
                            .build(ctx);
                            connect
                        }))
                        .with_orientation(Orientation::Horizontal)
                        .build(ctx),
                    ),
                )
                .add_column(Column::stretch())
                .add_row(Row::strict(20.0))
                .build(ctx),
            )
            .build(ctx);

        Self {
            window,
            sender,
            connect,
            selected: Default::default(),
        }
    }

    pub fn handle_message(&mut self, message: &UiMessage, editor_scene: &EditorScene) {
        scope_profile!();

        if let Some(ButtonMessage::Click) = message.data::<ButtonMessage>() {
            if message.destination() == self.connect {
                if let Selection::Navmesh(selection) = &editor_scene.selection {
                    let vertices = selection
                        .entities()
                        .iter()
                        .filter_map(|entity| {
                            if let NavmeshEntity::Edge(v) = *entity {
                                Some(v)
                            } else {
                                None
                            }
                        })
                        .collect::<Vec<_>>();

                    self.sender
                        .send(Message::do_scene_command(ConnectNavmeshEdgesCommand::new(
                            self.selected,
                            [vertices[0], vertices[1]],
                        )))
                        .unwrap();
                }
            }
        }
    }

    pub fn on_mode_changed(&mut self, ui: &UserInterface, mode: &Mode) {
        ui.send_message(WidgetMessage::enabled(
            window_content(self.window, ui),
            MessageDirection::ToWidget,
            mode.is_edit(),
        ));
    }
}

enum DragContext {
    MoveSelection {
        initial_positions: HashMap<usize, Vector3<f32>>,
    },
    EdgeDuplication {
        vertices: [PathVertex; 2],
        opposite_edge: TriangleEdge,
    },
}

impl DragContext {
    pub fn is_edge_duplication(&self) -> bool {
        matches!(self, DragContext::EdgeDuplication { .. })
    }
}

pub struct EditNavmeshMode {
    navmesh: Handle<Node>,
    move_gizmo: MoveGizmo,
    message_sender: Sender<Message>,
    drag_context: Option<DragContext>,
    plane_kind: PlaneKind,
}

impl EditNavmeshMode {
    pub fn new(
        editor_scene: &EditorScene,
        engine: &mut GameEngine,
        message_sender: Sender<Message>,
    ) -> Self {
        Self {
            navmesh: Default::default(),
            move_gizmo: MoveGizmo::new(editor_scene, engine),
            message_sender,
            drag_context: None,
            plane_kind: PlaneKind::X,
        }
    }
}

impl InteractionMode for EditNavmeshMode {
    fn on_left_mouse_button_down(
        &mut self,
        editor_scene: &mut EditorScene,
        engine: &mut GameEngine,
        mouse_pos: Vector2<f32>,
        frame_size: Vector2<f32>,
        settings: &Settings,
    ) {
        let scene = &mut engine.scenes[editor_scene.scene];
        let camera: &Camera = scene.graph[editor_scene.camera_controller.camera].as_camera();
        let ray = camera.make_ray(mouse_pos, frame_size);

        let camera = editor_scene.camera_controller.camera;
        let camera_pivot = editor_scene.camera_controller.pivot;
        let gizmo_origin = self.move_gizmo.origin;
        let editor_node = editor_scene
            .camera_controller
            .pick(PickingOptions {
                cursor_pos: mouse_pos,
                graph: &scene.graph,
                editor_objects_root: editor_scene.editor_objects_root,
                screen_size: frame_size,
                editor_only: true,
                filter: |handle, _| {
                    handle != camera && handle != camera_pivot && handle != gizmo_origin
                },
                ignore_back_faces: settings.selection.ignore_back_faces,
                use_picking_loop: true,
                only_meshes: false,
            })
            .map(|r| r.node)
            .unwrap_or_default();

        let graph = &mut engine.scenes[editor_scene.scene].graph;

        if let Some(plane_kind) = self.move_gizmo.handle_pick(editor_node, graph) {
            if let Some(navmesh) = graph
                .try_get_of_type::<NavigationalMesh>(self.navmesh)
                .map(|n| n.navmesh_ref())
            {
                let mut initial_positions = HashMap::new();
                for (index, vertex) in navmesh.vertices().iter().enumerate() {
                    initial_positions.insert(index, vertex.position);
                }
                self.plane_kind = plane_kind;
                self.drag_context = Some(DragContext::MoveSelection { initial_positions });
            }
        } else {
            if let Some(navmesh) = graph
                .try_get_of_type::<NavigationalMesh>(self.navmesh)
                .map(|n| n.navmesh_ref())
            {
                let mut new_selection = if engine.user_interface.keyboard_modifiers().shift {
                    if let Selection::Navmesh(navmesh_selection) = &editor_scene.selection {
                        navmesh_selection.clone()
                    } else {
                        NavmeshSelection::empty(self.navmesh)
                    }
                } else {
                    NavmeshSelection::empty(self.navmesh)
                };

                let mut picked = false;
                for (index, vertex) in navmesh.vertices().iter().enumerate() {
                    if ray
                        .sphere_intersection(&vertex.position, settings.navmesh.vertex_radius)
                        .is_some()
                    {
                        new_selection.add(NavmeshEntity::Vertex(index));
                        picked = true;
                        break;
                    }
                }

                if !picked {
                    for triangle in navmesh.triangles().iter() {
                        for edge in &triangle.edges() {
                            let begin = navmesh.vertices()[edge.a as usize].position;
                            let end = navmesh.vertices()[edge.b as usize].position;
                            if ray
                                .cylinder_intersection(
                                    &begin,
                                    &end,
                                    settings.navmesh.vertex_radius,
                                    CylinderKind::Finite,
                                )
                                .is_some()
                            {
                                new_selection.add(NavmeshEntity::Edge(*edge));
                                break;
                            }
                        }
                    }
                }

                let new_selection = Selection::Navmesh(new_selection);

                if new_selection != editor_scene.selection {
                    self.message_sender
                        .send(Message::do_scene_command(ChangeSelectionCommand::new(
                            new_selection,
                            editor_scene.selection.clone(),
                        )))
                        .unwrap();
                }
            }
        }
    }

    fn on_left_mouse_button_up(
        &mut self,
        editor_scene: &mut EditorScene,
        engine: &mut GameEngine,
        _mouse_pos: Vector2<f32>,
        _frame_size: Vector2<f32>,
        _settings: &Settings,
    ) {
        let graph = &mut engine.scenes[editor_scene.scene].graph;

        self.move_gizmo.reset_state(graph);

        if let Some(navmesh) = graph
            .try_get_of_type::<NavigationalMesh>(self.navmesh)
            .map(|n| n.navmesh_ref())
        {
            if let Some(drag_context) = self.drag_context.take() {
                let mut commands = Vec::new();

                match drag_context {
                    DragContext::MoveSelection { initial_positions } => {
                        if let Selection::Navmesh(navmesh_selection) = &mut editor_scene.selection {
                            for vertex in navmesh_selection.unique_vertices().iter() {
                                commands.push(SceneCommand::new(MoveNavmeshVertexCommand::new(
                                    self.navmesh,
                                    *vertex,
                                    *initial_positions.get(vertex).unwrap(),
                                    navmesh.vertices()[*vertex as usize].position,
                                )));
                            }
                        }
                    }
                    DragContext::EdgeDuplication {
                        vertices,
                        opposite_edge,
                    } => {
                        let va = vertices[0].clone();
                        let vb = vertices[1].clone();

                        commands.push(SceneCommand::new(AddNavmeshEdgeCommand::new(
                            self.navmesh,
                            (va, vb),
                            opposite_edge,
                            true,
                        )));
                    }
                }

                self.message_sender
                    .send(Message::do_scene_command(CommandGroup::from(commands)))
                    .unwrap();
            }
        }
    }

    fn on_mouse_move(
        &mut self,
        mouse_offset: Vector2<f32>,
        mouse_position: Vector2<f32>,
        camera: Handle<Node>,
        editor_scene: &mut EditorScene,
        engine: &mut GameEngine,
        frame_size: Vector2<f32>,
        _settings: &Settings,
    ) {
        if !self.drag_context.is_some() {
            return;
        }

        let offset = self.move_gizmo.calculate_offset(
            editor_scene,
            camera,
            mouse_offset,
            mouse_position,
            engine,
            frame_size,
            self.plane_kind,
        );

        let graph = &mut engine.scenes[editor_scene.scene].graph;

        if let Some(navmesh) = graph
            .try_get_mut_of_type::<NavigationalMesh>(self.navmesh)
            .map(|n| n.navmesh_mut())
        {
            // If we're dragging single edge it is possible to enter edge duplication mode by
            // holding Shift key. This is the main navmesh construction mode.
            if let Selection::Navmesh(navmesh_selection) = &editor_scene.selection {
                if navmesh_selection.entities().len() == 1 {
                    if let NavmeshEntity::Edge(edge) = navmesh_selection.entities().first().unwrap()
                    {
                        if engine.user_interface.keyboard_modifiers().shift
                            && !self.drag_context.as_ref().unwrap().is_edge_duplication()
                        {
                            let new_begin = navmesh.vertices()[edge.a as usize].clone();
                            let new_end = navmesh.vertices()[edge.b as usize].clone();

                            self.drag_context = Some(DragContext::EdgeDuplication {
                                vertices: [new_begin, new_end],
                                opposite_edge: *edge,
                            });

                            // Discard selection.
                            self.message_sender
                                .send(Message::do_scene_command(ChangeSelectionCommand::new(
                                    Selection::Navmesh(NavmeshSelection::empty(self.navmesh)),
                                    editor_scene.selection.clone(),
                                )))
                                .unwrap();
                        }
                    }
                }
            }

            if let Some(drag_context) = self.drag_context.as_mut() {
                match drag_context {
                    DragContext::MoveSelection { .. } => {
                        if let Selection::Navmesh(navmesh_selection) = &mut editor_scene.selection {
                            for &vertex in &*navmesh_selection.unique_vertices() {
                                navmesh.vertices_mut()[vertex].position += offset;
                            }
                        }
                    }
                    DragContext::EdgeDuplication { vertices, .. } => {
                        for vertex in vertices.iter_mut() {
                            vertex.position += offset;
                        }
                    }
                }
            }
        }
    }

    fn update(
        &mut self,
        editor_scene: &mut EditorScene,
        camera: Handle<Node>,
        engine: &mut GameEngine,
        settings: &Settings,
    ) {
        let scene = &mut engine.scenes[editor_scene.scene];
        self.move_gizmo.set_visible(&mut scene.graph, false);

        let scale = calculate_gizmo_distance_scaling(&scene.graph, camera, self.move_gizmo.origin);

        if let Some(navmesh) = scene
            .graph
            .try_get_mut_of_type::<NavigationalMesh>(self.navmesh)
            .map(|n| n.navmesh_mut())
        {
            let mut gizmo_visible = false;
            let mut gizmo_position = Default::default();

            if let Some(DragContext::EdgeDuplication {
                vertices,
                opposite_edge,
            }) = self.drag_context.as_ref()
            {
                for vertex in vertices.iter() {
                    scene.drawing_context.draw_sphere(
                        vertex.position,
                        10,
                        10,
                        settings.navmesh.vertex_radius,
                        Color::RED,
                    );
                }

                let ob = navmesh.vertices()[opposite_edge.a as usize].position;
                let nb = vertices[0].position;
                let oe = navmesh.vertices()[opposite_edge.b as usize].position;
                let ne = vertices[1].position;

                scene.drawing_context.add_line(fyrox::scene::debug::Line {
                    begin: nb,
                    end: ne,
                    color: Color::RED,
                });

                for &(begin, end) in &[(ob, oe), (ob, nb), (nb, oe), (oe, ne)] {
                    scene.drawing_context.add_line(fyrox::scene::debug::Line {
                        begin,
                        end,
                        color: Color::GREEN,
                    });
                }

                gizmo_visible = true;
                gizmo_position = (nb + ne).scale(0.5);
            }

            if let Selection::Navmesh(navmesh_selection) = &editor_scene.selection {
                if let Some(first) = navmesh_selection.first() {
                    gizmo_visible = true;
                    gizmo_position = match *first {
                        NavmeshEntity::Vertex(v) => navmesh.vertices()[v].position,
                        NavmeshEntity::Edge(edge) => {
                            let a = navmesh.vertices()[edge.a as usize].position;
                            let b = navmesh.vertices()[edge.b as usize].position;
                            (a + b).scale(0.5)
                        }
                    };
                }
            }

            self.move_gizmo.set_visible(&mut scene.graph, gizmo_visible);
            self.move_gizmo
                .transform(&mut scene.graph)
                .set_scale(scale)
                .set_position(gizmo_position);
        }
    }

    fn deactivate(&mut self, editor_scene: &EditorScene, engine: &mut GameEngine) {
        let scene = &mut engine.scenes[editor_scene.scene];
        self.move_gizmo.set_visible(&mut scene.graph, false);
    }

    fn on_key_down(
        &mut self,
        key: KeyCode,
        editor_scene: &mut EditorScene,
        engine: &mut GameEngine,
    ) -> bool {
        let scene = &mut engine.scenes[editor_scene.scene];

        match key {
            KeyCode::Delete => {
                if scene
                    .graph
                    .try_get_of_type::<NavigationalMesh>(self.navmesh)
                    .map(|n| n.navmesh_ref())
                    .is_some()
                {
                    if let Selection::Navmesh(navmesh_selection) = &mut editor_scene.selection {
                        if !navmesh_selection.is_empty() {
                            let mut commands = Vec::new();

                            for &vertex in &*navmesh_selection.unique_vertices() {
                                commands.push(SceneCommand::new(DeleteNavmeshVertexCommand::new(
                                    self.navmesh,
                                    vertex,
                                )));
                            }

                            commands.push(SceneCommand::new(ChangeSelectionCommand::new(
                                Selection::Navmesh(NavmeshSelection::empty(self.navmesh)),
                                editor_scene.selection.clone(),
                            )));

                            self.message_sender
                                .send(Message::do_scene_command(CommandGroup::from(commands)))
                                .unwrap();
                        }
                    }
                }

                true
            }
            KeyCode::A if engine.user_interface.keyboard_modifiers().control => {
                if let Some(navmesh) = scene
                    .graph
                    .try_get_of_type::<NavigationalMesh>(self.navmesh)
                    .map(|n| n.navmesh_ref())
                {
                    let selection = NavmeshSelection::new(
                        self.navmesh,
                        navmesh
                            .vertices()
                            .iter()
                            .enumerate()
                            .map(|(handle, _)| NavmeshEntity::Vertex(handle))
                            .collect(),
                    );

                    self.message_sender
                        .send(Message::do_scene_command(ChangeSelectionCommand::new(
                            Selection::Navmesh(selection),
                            editor_scene.selection.clone(),
                        )))
                        .unwrap();
                }

                true
            }
            _ => false,
        }
    }
}
