use crate::{
    inspector::editors::make_property_editors_container,
    message::MessageSender,
    scene::{
        commands::{graph::AddModelCommand, ChangeSelectionCommand, CommandGroup, SceneCommand},
        EditorScene, Selection,
    },
    world::graph::selection::GraphSelection,
    MSG_SYNC_FLAG,
};
use fyrox::{
    core::{
        algebra::{UnitQuaternion, Vector3},
        log::Log,
        math::Matrix4Ext,
        pool::Handle,
        reflect::prelude::*,
    },
    gui::{
        button::{ButtonBuilder, ButtonMessage},
        grid::{Column, GridBuilder, Row},
        inspector::{InspectorBuilder, InspectorContext, InspectorMessage, PropertyAction},
        message::{MessageDirection, UiMessage},
        stack_panel::StackPanelBuilder,
        widget::WidgetBuilder,
        window::{WindowBuilder, WindowMessage, WindowTitle},
        BuildContext, HorizontalAlignment, Orientation, Thickness, UiNode, UserInterface,
    },
    scene::{
        base::BaseBuilder,
        collider::{ColliderBuilder, ColliderShape},
        graph::Graph,
        joint::{BallJoint, JointBuilder, JointParams, RevoluteJoint},
        node::Node,
        ragdoll::{Limb, RagdollBuilder},
        rigidbody::{RigidBodyBuilder, RigidBodyType},
        transform::TransformBuilder,
    },
};
use std::{ops::Range, rc::Rc};

#[derive(Reflect, Debug)]
pub struct RagdollPreset {
    hips: Handle<Node>,
    left_up_leg: Handle<Node>,
    left_leg: Handle<Node>,
    left_foot: Handle<Node>,
    right_up_leg: Handle<Node>,
    right_leg: Handle<Node>,
    right_foot: Handle<Node>,
    spine: Handle<Node>,
    spine1: Handle<Node>,
    spine2: Handle<Node>,
    left_shoulder: Handle<Node>,
    left_arm: Handle<Node>,
    left_fore_arm: Handle<Node>,
    left_hand: Handle<Node>,
    right_shoulder: Handle<Node>,
    right_arm: Handle<Node>,
    right_fore_arm: Handle<Node>,
    right_hand: Handle<Node>,
    neck: Handle<Node>,
    head: Handle<Node>,
    total_mass: f32,
    friction: f32,
    use_ccd: bool,
}

impl Default for RagdollPreset {
    fn default() -> Self {
        Self {
            hips: Default::default(),
            left_up_leg: Default::default(),
            left_leg: Default::default(),
            left_foot: Default::default(),
            right_up_leg: Default::default(),
            right_leg: Default::default(),
            right_foot: Default::default(),
            spine: Default::default(),
            spine1: Default::default(),
            spine2: Default::default(),
            left_shoulder: Default::default(),
            left_arm: Default::default(),
            left_fore_arm: Default::default(),
            left_hand: Default::default(),
            right_shoulder: Default::default(),
            right_arm: Default::default(),
            right_fore_arm: Default::default(),
            right_hand: Default::default(),
            neck: Default::default(),
            head: Default::default(),
            total_mass: 20.0,
            friction: 0.5,
            use_ccd: true,
        }
    }
}

fn try_make_ball_joint(
    body1: Handle<Node>,
    body2: Handle<Node>,
    name: &str,
    limits: Option<Range<f32>>,
    ragdoll: Handle<Node>,
    graph: &mut Graph,
) -> Handle<Node> {
    if body1.is_some() && body2.is_some() {
        let mut joint = BallJoint::default();

        if let Some(limits) = limits {
            // Just form a solid angle.
            joint.x_limits_enabled = true;
            joint.y_limits_enabled = true;
            joint.z_limits_enabled = true;

            joint.x_limits_angles = limits.clone();
            joint.y_limits_angles = limits.clone();
            joint.z_limits_angles = limits;
        }

        let ball_joint = JointBuilder::new(
            BaseBuilder::new().with_name(name).with_local_transform(
                TransformBuilder::new()
                    .with_local_position(graph[body1].global_position())
                    .with_local_rotation(UnitQuaternion::from_matrix_eps(
                        &graph[body1].global_transform().basis(),
                        f32::EPSILON,
                        16,
                        Default::default(),
                    ))
                    .build(),
            ),
        )
        .with_params(JointParams::BallJoint(joint))
        .with_body1(body1)
        .with_body2(body2)
        .with_auto_rebinding_enabled(false)
        .with_contacts_enabled(false)
        .build(graph);

        graph.link_nodes(ball_joint, ragdoll);

        ball_joint
    } else {
        Default::default()
    }
}

fn try_make_hinge_joint(
    body1: Handle<Node>,
    body2: Handle<Node>,
    name: &str,
    limits: Option<Range<f32>>,
    ragdoll: Handle<Node>,
    graph: &mut Graph,
) -> Handle<Node> {
    if body1.is_some() && body2.is_some() {
        let mut joint = RevoluteJoint::default();

        if let Some(limits) = limits {
            joint.limits_enabled = true;
            joint.limits = limits;
        }

        let hinge_joint = JointBuilder::new(
            BaseBuilder::new().with_name(name).with_local_transform(
                TransformBuilder::new()
                    .with_local_position(graph[body1].global_position())
                    .with_local_rotation(UnitQuaternion::from_matrix_eps(
                        &graph[body1].global_transform().basis(),
                        f32::EPSILON,
                        16,
                        Default::default(),
                    ))
                    .build(),
            ),
        )
        .with_params(JointParams::RevoluteJoint(joint))
        .with_body1(body1)
        .with_body2(body2)
        .with_auto_rebinding_enabled(false)
        .with_contacts_enabled(false)
        .build(graph);

        graph.link_nodes(hinge_joint, ragdoll);

        hinge_joint
    } else {
        Default::default()
    }
}

impl RagdollPreset {
    fn make_sphere(
        &self,
        from: Handle<Node>,
        radius: f32,
        name: &str,
        ragdoll: Handle<Node>,
        apply_offset: bool,
        graph: &mut Graph,
    ) -> Handle<Node> {
        if let Some(from_ref) = graph.try_get(from) {
            let offset = if apply_offset {
                from_ref
                    .up_vector()
                    .try_normalize(f32::EPSILON)
                    .unwrap_or_default()
                    .scale(radius)
            } else {
                Default::default()
            };

            let sphere = RigidBodyBuilder::new(
                BaseBuilder::new()
                    .with_name(name)
                    .with_local_transform(
                        TransformBuilder::new()
                            .with_local_position(from_ref.global_position() + offset)
                            .build(),
                    )
                    .with_children(&[ColliderBuilder::new(
                        BaseBuilder::new().with_name("SphereCollider"),
                    )
                    .with_friction(self.friction)
                    .with_shape(ColliderShape::ball(radius))
                    .build(graph)]),
            )
            .with_ccd_enabled(self.use_ccd)
            .with_body_type(RigidBodyType::KinematicPositionBased)
            .build(graph);

            graph.link_nodes(sphere, ragdoll);

            sphere
        } else {
            Default::default()
        }
    }

    fn make_oriented_capsule(
        &self,
        from: Handle<Node>,
        to: Handle<Node>,
        radius: f32,
        name: &str,
        ragdoll: Handle<Node>,
        graph: &mut Graph,
    ) -> Handle<Node> {
        if let (Some(from_ref), Some(to_ref)) = (graph.try_get(from), graph.try_get(to)) {
            let pos_from = from_ref.global_position();
            let pos_to = to_ref.global_position();

            let capsule = RigidBodyBuilder::new(
                BaseBuilder::new()
                    .with_name(name)
                    .with_local_transform(
                        TransformBuilder::new()
                            .with_local_position(pos_from)
                            .with_local_rotation(UnitQuaternion::from_matrix_eps(
                                &from_ref.global_transform().basis(),
                                f32::EPSILON,
                                16,
                                Default::default(),
                            ))
                            .build(),
                    )
                    .with_children(&[ColliderBuilder::new(
                        BaseBuilder::new().with_name("CapsuleCollider"),
                    )
                    .with_shape(ColliderShape::capsule(
                        Vector3::default(),
                        Vector3::new(0.0, (pos_to - pos_from).norm() - 2.0 * radius, 0.0),
                        radius,
                    ))
                    .with_friction(self.friction)
                    .build(graph)]),
            )
            .with_ccd_enabled(self.use_ccd)
            .with_body_type(RigidBodyType::KinematicPositionBased)
            .build(graph);

            graph.link_nodes(capsule, ragdoll);

            capsule
        } else {
            Default::default()
        }
    }

    fn make_cuboid(
        &self,
        from: Handle<Node>,
        half_size: Vector3<f32>,
        name: &str,
        ragdoll: Handle<Node>,
        graph: &mut Graph,
    ) -> Handle<Node> {
        if let Some(from_ref) = graph.try_get(from) {
            let cuboid = RigidBodyBuilder::new(
                BaseBuilder::new()
                    .with_name(name)
                    .with_local_transform(
                        TransformBuilder::new()
                            .with_local_position(from_ref.global_position())
                            .build(),
                    )
                    .with_children(&[ColliderBuilder::new(
                        BaseBuilder::new().with_name("CuboidCollider"),
                    )
                    .with_shape(ColliderShape::cuboid(half_size.x, half_size.y, half_size.z))
                    .with_friction(self.friction)
                    .build(graph)]),
            )
            .with_ccd_enabled(self.use_ccd)
            .with_body_type(RigidBodyType::KinematicPositionBased)
            .build(graph);

            graph.link_nodes(cuboid, ragdoll);

            cuboid
        } else {
            Default::default()
        }
    }

    /// Calculates base size (size of the head) using common human body proportions. It uses distance between hand and elbow as a
    /// head size (it matches 1:1).
    fn measure_base_size(&self, graph: &Graph) -> f32 {
        let mut base_size = 0.2;
        for (upper, lower) in [
            (self.left_fore_arm, self.left_hand),
            (self.left_fore_arm, self.right_hand),
        ] {
            if let (Some(upper_ref), Some(lower_ref)) = (graph.try_get(upper), graph.try_get(lower))
            {
                base_size = (upper_ref.global_position() - lower_ref.global_position()).norm();
                break;
            }
        }
        base_size
    }

    pub fn create_and_send_command(
        &self,
        graph: &mut Graph,
        editor_scene: &EditorScene,
        sender: &MessageSender,
    ) {
        let base_size = self.measure_base_size(graph);

        let ragdoll = RagdollBuilder::new(BaseBuilder::new().with_name("Ragdoll"))
            .with_active(true)
            .build(graph);

        graph.link_nodes(ragdoll, editor_scene.scene_content_root);

        let left_up_leg = self.make_oriented_capsule(
            self.left_up_leg,
            self.left_leg,
            0.35 * base_size,
            "RagdollLeftUpLeg",
            ragdoll,
            graph,
        );

        let left_leg = self.make_oriented_capsule(
            self.left_leg,
            self.left_foot,
            0.3 * base_size,
            "RagdollLeftLeg",
            ragdoll,
            graph,
        );

        let left_foot = self.make_sphere(
            self.left_foot,
            0.2 * base_size,
            "RagdollLeftFoot",
            ragdoll,
            false,
            graph,
        );

        let right_up_leg = self.make_oriented_capsule(
            self.right_up_leg,
            self.right_leg,
            0.35 * base_size,
            "RagdollRightUpLeg",
            ragdoll,
            graph,
        );

        let right_leg = self.make_oriented_capsule(
            self.right_leg,
            self.right_foot,
            0.3 * base_size,
            "RagdollRightLeg",
            ragdoll,
            graph,
        );

        let right_foot = self.make_sphere(
            self.right_foot,
            0.2 * base_size,
            "RagdollRightFoot",
            ragdoll,
            false,
            graph,
        );

        let hips = self.make_cuboid(
            self.hips,
            Vector3::new(base_size * 0.5, base_size * 0.2, base_size * 0.4),
            "RagdollHips",
            ragdoll,
            graph,
        );

        let spine = self.make_cuboid(
            self.spine,
            Vector3::new(base_size * 0.45, base_size * 0.2, base_size * 0.4),
            "RagdollSpine",
            ragdoll,
            graph,
        );

        let spine1 = self.make_cuboid(
            self.spine1,
            Vector3::new(base_size * 0.45, base_size * 0.2, base_size * 0.4),
            "RagdollSpine1",
            ragdoll,
            graph,
        );

        let spine2 = self.make_cuboid(
            self.spine2,
            Vector3::new(base_size * 0.45, base_size * 0.2, base_size * 0.4),
            "RagdollSpine2",
            ragdoll,
            graph,
        );

        // Left arm.
        let left_shoulder = self.make_oriented_capsule(
            self.left_shoulder,
            self.left_arm,
            0.2 * base_size,
            "RagdollLeftShoulder",
            ragdoll,
            graph,
        );

        let left_arm = self.make_oriented_capsule(
            self.left_arm,
            self.left_fore_arm,
            0.2 * base_size,
            "RagdollLeftArm",
            ragdoll,
            graph,
        );

        let left_fore_arm = self.make_oriented_capsule(
            self.left_fore_arm,
            self.left_hand,
            0.2 * base_size,
            "RagdollLeftForeArm",
            ragdoll,
            graph,
        );

        let left_hand = self.make_sphere(
            self.left_hand,
            0.3 * base_size,
            "LeftHand",
            ragdoll,
            false,
            graph,
        );

        // Right arm.
        let right_shoulder = self.make_oriented_capsule(
            self.right_shoulder,
            self.right_arm,
            0.2 * base_size,
            "RagdollRightShoulder",
            ragdoll,
            graph,
        );

        let right_arm = self.make_oriented_capsule(
            self.right_arm,
            self.right_fore_arm,
            0.2 * base_size,
            "RagdollRightArm",
            ragdoll,
            graph,
        );

        let right_fore_arm = self.make_oriented_capsule(
            self.right_fore_arm,
            self.right_hand,
            0.2 * base_size,
            "RagdollRightForeArm",
            ragdoll,
            graph,
        );

        let right_hand = self.make_sphere(
            self.right_hand,
            0.3 * base_size,
            "RightHand",
            ragdoll,
            false,
            graph,
        );

        let neck = self.make_oriented_capsule(
            self.neck,
            self.head,
            0.2 * base_size,
            "RagdollNeck",
            ragdoll,
            graph,
        );

        let head = self.make_sphere(
            self.head,
            0.5 * base_size,
            "RightHand",
            ragdoll,
            true,
            graph,
        );

        // Link limbs with joints.
        graph.update_hierarchical_data();

        // Left leg.
        try_make_ball_joint(
            left_up_leg,
            hips,
            "RagdollLeftUpLegHipsBallJoint",
            Some(-80.0f32.to_radians()..80.0f32.to_radians()),
            ragdoll,
            graph,
        );
        try_make_hinge_joint(
            left_leg,
            left_up_leg,
            "RagdollLeftLegLeftUpLegHingeJoint",
            None,
            ragdoll,
            graph,
        );
        try_make_hinge_joint(
            left_foot,
            left_leg,
            "RagdollLeftFootLeftLegHingeJoint",
            Some(-45.0f32.to_radians()..45.0f32.to_radians()),
            ragdoll,
            graph,
        );

        // Right leg.
        try_make_ball_joint(
            right_up_leg,
            hips,
            "RagdollLeftUpLegHipsBallJoint",
            Some(-80.0f32.to_radians()..80.0f32.to_radians()),
            ragdoll,
            graph,
        );
        try_make_hinge_joint(
            right_leg,
            right_up_leg,
            "RagdollRightLegRightUpLegHingeJoint",
            None,
            ragdoll,
            graph,
        );
        try_make_hinge_joint(
            right_foot,
            right_leg,
            "RagdollRightFootRightLegHingeJoint",
            Some(-45.0f32.to_radians()..45.0f32.to_radians()),
            ragdoll,
            graph,
        );

        try_make_hinge_joint(
            spine,
            hips,
            "RagdollSpineHipsHingeJoint",
            None,
            ragdoll,
            graph,
        );

        try_make_hinge_joint(
            spine1,
            spine,
            "RagdollSpine1SpineHingeJoint",
            None,
            ragdoll,
            graph,
        );

        try_make_hinge_joint(
            spine2,
            spine1,
            "RagdollSpine2Spine1HingeJoint",
            None,
            ragdoll,
            graph,
        );

        try_make_hinge_joint(
            left_shoulder,
            spine2,
            "RagdollSpine2LeftShoulderBallJoint",
            None,
            ragdoll,
            graph,
        );
        try_make_ball_joint(
            left_arm,
            left_shoulder,
            "RagdollLeftShoulderLeftArmBallJoint",
            None,
            ragdoll,
            graph,
        );
        try_make_hinge_joint(
            left_fore_arm,
            left_arm,
            "RagdollLeftArmLeftForeArmBallJoint",
            None,
            ragdoll,
            graph,
        );
        try_make_ball_joint(
            left_hand,
            left_fore_arm,
            "RagdollLeftForeArmLeftHandBallJoint",
            Some(-45.0f32.to_radians()..45.0f32.to_radians()),
            ragdoll,
            graph,
        );

        try_make_hinge_joint(
            right_shoulder,
            spine2,
            "RagdollSpine2RightShoulderBallJoint",
            None,
            ragdoll,
            graph,
        );
        try_make_ball_joint(
            right_arm,
            right_shoulder,
            "RagdollRightShoulderRightArmBallJoint",
            None,
            ragdoll,
            graph,
        );
        try_make_hinge_joint(
            right_fore_arm,
            right_arm,
            "RagdollRightArmRightForeArmHingeJoint",
            None,
            ragdoll,
            graph,
        );
        try_make_ball_joint(
            right_hand,
            right_fore_arm,
            "RagdollRightForeArmRightHandBallJoint",
            Some(-45.0f32.to_radians()..45.0f32.to_radians()),
            ragdoll,
            graph,
        );

        try_make_ball_joint(
            neck,
            spine2,
            "RagdollNeckSpine2BallJoint",
            None,
            ragdoll,
            graph,
        );
        try_make_ball_joint(head, neck, "RagdollHeadNeckBallJoint", None, ragdoll, graph);

        graph[ragdoll].as_ragdoll_mut().set_hips(Limb {
            bone: self.hips,
            physical_bone: hips,
            children: vec![
                Limb {
                    bone: self.spine,
                    physical_bone: spine,
                    children: vec![Limb {
                        bone: self.spine1,
                        physical_bone: spine1,
                        children: vec![Limb {
                            bone: self.spine2,
                            physical_bone: spine2,
                            children: vec![
                                Limb {
                                    bone: self.left_shoulder,
                                    physical_bone: left_shoulder,
                                    children: vec![Limb {
                                        bone: self.left_arm,
                                        physical_bone: left_arm,
                                        children: vec![Limb {
                                            bone: self.left_fore_arm,
                                            physical_bone: left_fore_arm,
                                            children: vec![Limb {
                                                bone: self.left_hand,
                                                physical_bone: left_hand,
                                                children: vec![],
                                            }],
                                        }],
                                    }],
                                },
                                Limb {
                                    bone: self.right_shoulder,
                                    physical_bone: right_shoulder,
                                    children: vec![Limb {
                                        bone: self.right_arm,
                                        physical_bone: right_arm,
                                        children: vec![Limb {
                                            bone: self.right_fore_arm,
                                            physical_bone: right_fore_arm,
                                            children: vec![Limb {
                                                bone: self.right_hand,
                                                physical_bone: right_hand,
                                                children: vec![],
                                            }],
                                        }],
                                    }],
                                },
                                Limb {
                                    bone: self.neck,
                                    physical_bone: neck,
                                    children: vec![Limb {
                                        bone: self.head,
                                        physical_bone: head,
                                        children: vec![],
                                    }],
                                },
                            ],
                        }],
                    }],
                },
                Limb {
                    bone: self.left_up_leg,
                    physical_bone: left_up_leg,
                    children: vec![Limb {
                        bone: self.left_leg,
                        physical_bone: left_leg,
                        children: vec![Limb {
                            bone: self.left_foot,
                            physical_bone: left_foot,
                            children: vec![],
                        }],
                    }],
                },
                Limb {
                    bone: self.right_up_leg,
                    physical_bone: right_up_leg,
                    children: vec![Limb {
                        bone: self.right_leg,
                        physical_bone: right_leg,
                        children: vec![Limb {
                            bone: self.right_foot,
                            physical_bone: right_foot,
                            children: vec![],
                        }],
                    }],
                },
            ],
        });

        // Immediately after extract if from the scene to subgraph. This is required to not violate
        // the rule of one place of execution, only commands allowed to modify the scene.
        let sub_graph = graph.take_reserve_sub_graph(ragdoll);

        let group = vec![
            SceneCommand::new(AddModelCommand::new(sub_graph)),
            // We also want to select newly instantiated model.
            SceneCommand::new(ChangeSelectionCommand::new(
                Selection::Graph(GraphSelection::single_or_empty(ragdoll)),
                editor_scene.selection.clone(),
            )),
        ];

        sender.do_scene_command(CommandGroup::from(group).with_custom_name("Generate Ragdoll"));
    }
}

pub struct RagdollWizard {
    pub window: Handle<UiNode>,
    pub preset: RagdollPreset,
    inspector: Handle<UiNode>,
    ok: Handle<UiNode>,
    cancel: Handle<UiNode>,
    autofill: Handle<UiNode>,
}

impl RagdollWizard {
    pub fn new(ctx: &mut BuildContext, sender: MessageSender) -> Self {
        let preset = RagdollPreset::default();
        let container = Rc::new(make_property_editors_container(sender));

        let inspector;
        let ok;
        let cancel;
        let autofill;
        let window = WindowBuilder::new(
            WidgetBuilder::new()
                .with_width(350.0)
                .with_height(550.0)
                .with_name("RagdollWizard"),
        )
        .open(false)
        .with_title(WindowTitle::text("Ragdoll Wizard"))
        .with_content(
            GridBuilder::new(
                WidgetBuilder::new()
                    .with_child({
                        inspector = InspectorBuilder::new(
                            WidgetBuilder::new().with_margin(Thickness::uniform(1.0)),
                        )
                        .with_context(InspectorContext::from_object(
                            &preset,
                            ctx,
                            container,
                            None,
                            MSG_SYNC_FLAG,
                            0,
                            true,
                            Default::default(),
                        ))
                        .build(ctx);
                        inspector
                    })
                    .with_child(
                        StackPanelBuilder::new(
                            WidgetBuilder::new()
                                .with_horizontal_alignment(HorizontalAlignment::Right)
                                .on_row(1)
                                .with_margin(Thickness::uniform(1.0))
                                .with_child({
                                    autofill = ButtonBuilder::new(
                                        WidgetBuilder::new()
                                            .with_width(100.0)
                                            .with_margin(Thickness::uniform(1.0)),
                                    )
                                    .with_text("Autofill")
                                    .build(ctx);
                                    autofill
                                })
                                .with_child({
                                    ok = ButtonBuilder::new(
                                        WidgetBuilder::new()
                                            .with_width(100.0)
                                            .with_margin(Thickness::uniform(1.0)),
                                    )
                                    .with_text("OK")
                                    .build(ctx);
                                    ok
                                })
                                .with_child({
                                    cancel = ButtonBuilder::new(
                                        WidgetBuilder::new()
                                            .with_width(100.0)
                                            .with_margin(Thickness::uniform(1.0)),
                                    )
                                    .with_text("Cancel")
                                    .build(ctx);
                                    cancel
                                }),
                        )
                        .with_orientation(Orientation::Horizontal)
                        .build(ctx),
                    ),
            )
            .add_row(Row::stretch())
            .add_row(Row::strict(24.0))
            .add_column(Column::stretch())
            .build(ctx),
        )
        .build(ctx);

        Self {
            window,
            preset,
            inspector,
            ok,
            cancel,
            autofill,
        }
    }

    pub fn open(&self, ui: &UserInterface) {
        ui.send_message(WindowMessage::open(
            self.window,
            MessageDirection::ToWidget,
            true,
        ));
    }

    pub fn handle_ui_message(
        &mut self,
        message: &UiMessage,
        ui: &mut UserInterface,
        graph: &mut Graph,
        editor_scene: &EditorScene,
        sender: &MessageSender,
    ) {
        if let Some(InspectorMessage::PropertyChanged(args)) = message.data() {
            if message.destination() == self.inspector
                && message.direction() == MessageDirection::FromWidget
            {
                PropertyAction::from_field_kind(&args.value).apply(
                    &args.path(),
                    &mut self.preset,
                    &mut |result| {
                        Log::verify(result);
                    },
                );
            }
        } else if let Some(ButtonMessage::Click) = message.data() {
            if message.destination() == self.ok {
                self.preset
                    .create_and_send_command(graph, editor_scene, sender);

                ui.send_message(WindowMessage::close(
                    self.window,
                    MessageDirection::ToWidget,
                ));
            } else if message.destination() == self.cancel {
                ui.send_message(WindowMessage::close(
                    self.window,
                    MessageDirection::ToWidget,
                ));
            } else if message.destination() == self.autofill {
                fn find_by_pattern(graph: &Graph, pattern: &str) -> Handle<Node> {
                    graph
                        .find(graph.get_root(), &mut |n| n.name().contains(pattern))
                        .map(|(h, _)| h)
                        .unwrap_or_default()
                }

                self.preset.hips = find_by_pattern(graph, "Hips");

                self.preset.spine = find_by_pattern(graph, "Spine");
                self.preset.spine1 = find_by_pattern(graph, "Spine1");
                self.preset.spine2 = find_by_pattern(graph, "Spine2");

                self.preset.right_up_leg = find_by_pattern(graph, "RightUpLeg");
                self.preset.right_leg = find_by_pattern(graph, "RightLeg");
                self.preset.right_foot = find_by_pattern(graph, "RightFoot");

                self.preset.left_up_leg = find_by_pattern(graph, "LeftUpLeg");
                self.preset.left_leg = find_by_pattern(graph, "LeftLeg");
                self.preset.left_foot = find_by_pattern(graph, "LeftFoot");

                self.preset.right_hand = find_by_pattern(graph, "RightHand");
                self.preset.right_arm = find_by_pattern(graph, "RightArm");
                self.preset.right_fore_arm = find_by_pattern(graph, "RightForeArm");
                self.preset.right_shoulder = find_by_pattern(graph, "RightShoulder");

                self.preset.left_hand = find_by_pattern(graph, "LeftHand");
                self.preset.left_arm = find_by_pattern(graph, "LeftArm");
                self.preset.left_fore_arm = find_by_pattern(graph, "LeftForeArm");
                self.preset.left_shoulder = find_by_pattern(graph, "LeftShoulder");

                self.preset.neck = find_by_pattern(graph, "Neck");
                self.preset.head = find_by_pattern(graph, "Head");

                let ctx = ui
                    .node(self.inspector)
                    .cast::<fyrox::gui::inspector::Inspector>()
                    .unwrap()
                    .context()
                    .clone();

                if let Err(sync_errors) = ctx.sync(&self.preset, ui, 0, true, Default::default()) {
                    for error in sync_errors {
                        Log::err(format!("Failed to sync property. Reason: {:?}", error))
                    }
                }
            }
        }
    }
}
