#[macro_export]
macro_rules! define_universal_commands {
    ($name:ident, $command:ident, $command_wrapper:ty, $ctx:ty, $handle:ty, $ctx_ident:ident, $handle_ident:ident, $self:ident, $entity_getter:block, $($field_name:ident: $field_type:ty),*) => {
        pub fn $name($handle_ident: $handle, property_changed: &fyrox::gui::inspector::PropertyChanged, $($field_name: $field_type),*) -> Option<$command_wrapper> {
            match fyrox::gui::inspector::PropertyAction::from_field_kind(&property_changed.value) {
                fyrox::gui::inspector::PropertyAction::Modify { value } => Some(<$command_wrapper>::new(SetPropertyCommand::new(
                    $handle_ident,
                    property_changed.path(),
                    value,
                    $($field_name),*
                ))),
                fyrox::gui::inspector::PropertyAction::AddItem { value } => Some(<$command_wrapper>::new(
                    AddCollectionItemCommand::new($handle_ident, property_changed.path(), value, $($field_name),*)
                )),
                fyrox::gui::inspector::PropertyAction::RemoveItem { index } => Some(<$command_wrapper>::new(
                    RemoveCollectionItemCommand::new($handle_ident, property_changed.path(), index, $($field_name),*)
                )),
                // Must be handled outside, there is not enough context and it near to impossible to create universal reversion
                // for InheritableVariable<T>.
                fyrox::gui::inspector::PropertyAction::Revert => None
            }
        }

        fn try_modify_property<F: FnOnce(&mut dyn fyrox::core::reflect::Reflect)>(
            entity: &mut dyn fyrox::core::reflect::Reflect,
            path: &str,
            func: F,
        ) {
            let mut func = Some(func);
             entity.resolve_path_mut(path, &mut |result| match  result {
                Ok(field) => func.take().unwrap()(field),
                Err(e) => fyrox::core::log::Log::err(format!(
                    "There is no such property {}! Reason: {:?}",
                    path, e
                )),
            })
        }

        #[derive(Debug)]
        pub struct SetPropertyCommand {
            #[allow(dead_code)]
            $handle_ident: $handle,
            value: Option<Box<dyn fyrox::core::reflect::Reflect>>,
            path: String,
            $($field_name: $field_type),*
        }

        impl SetPropertyCommand {
            pub fn new($handle_ident: $handle, path: String, value: Box<dyn fyrox::core::reflect::Reflect>, $($field_name: $field_type),*) -> Self {
                Self {
                    $handle_ident,
                    value: Some(value),
                    path,
                    $($field_name),*
                }
            }

            fn swap(&mut $self, $ctx_ident: &mut $ctx) {

                (($entity_getter) as &mut dyn Reflect).set_field_by_path(&$self.path, $self.value.take().unwrap(), &mut |result| match result {
                    Ok(old_value) => {
                        $self.value = Some(old_value);
                    }
                    Err(result) => {
                        let value = match result {
                            SetFieldByPathError::InvalidPath { value, reason } => {
                                fyrox::core::log::Log::err(format!(
                                    "Failed to set property {}! Invalid path {:?}!",
                                    $self.path, reason
                                ));

                                value
                            },
                            SetFieldByPathError::InvalidValue(value) => {
                                fyrox::core::log::Log::err(format!(
                                    "Failed to set property {}! Incompatible types!",
                                    $self.path
                                ));

                                value
                            }
                        };
                        $self.value = Some(value);

                    }
                });
            }
        }

        impl $command for SetPropertyCommand {
            fn name(&mut $self, _: &$ctx) -> String {
                format!("Set {} property", $self.path)
            }

            fn execute(&mut $self, $ctx_ident: &mut $ctx) {
                $self.swap($ctx_ident);
            }

            fn revert(&mut $self, $ctx_ident: &mut $ctx) {
                $self.swap($ctx_ident);
            }
        }

        #[derive(Debug)]
        pub struct AddCollectionItemCommand {
            #[allow(dead_code)]
            $handle_ident: $handle,
            path: String,
            item: Option<Box<dyn fyrox::core::reflect::Reflect>>,
            $($field_name: $field_type),*
        }

        impl AddCollectionItemCommand {
            pub fn new($handle_ident: $handle, path: String, item: Box<dyn fyrox::core::reflect::Reflect>, $($field_name: $field_type),*) -> Self {
                Self {
                    $handle_ident,
                    path,
                    item: Some(item),
                    $($field_name),*
                }
            }
        }

        impl $command for AddCollectionItemCommand {
            fn name(&mut $self, _: &$ctx) -> String {
                format!("Add item to {} collection", $self.path)
            }

            fn execute(&mut $self, $ctx_ident: &mut $ctx) {
                try_modify_property($entity_getter, &$self.path, |field| {
                    field.as_list_mut(&mut |result| {
                        if let Some(list) = result {
                            if let Err(item) = list.reflect_push($self.item.take().unwrap()) {
                                fyrox::core::log::Log::err(format!(
                                    "Failed to push item to {} collection. Type mismatch {} and {}!",
                                    $self.path, item.type_name(), list.type_name()
                                ));
                                $self.item = Some(item);
                            }
                        } else {
                            fyrox::core::log::Log::err(format!("Property {} is not a collection!", $self.path))
                        }
                    });
                })
            }

            fn revert(&mut $self, $ctx_ident: &mut $ctx) {
                try_modify_property($entity_getter, &$self.path, |field| {
                    field.as_list_mut(&mut |result| {
                        if let Some(list) = result {
                            if let Some(item) = list.reflect_pop() {
                                $self.item = Some(item);
                            } else {
                                fyrox::core::log::Log::err(format!("Failed to pop item from {} collection!", $self.path))
                            }
                        } else {
                            fyrox::core::log::Log::err(format!("Property {} is not a collection!", $self.path))
                        }
                    });
                })
            }
        }

        #[derive(Debug)]
        pub struct RemoveCollectionItemCommand {
            #[allow(dead_code)]
            $handle_ident: $handle,
            path: String,
            index: usize,
            value: Option<Box<dyn fyrox::core::reflect::Reflect>>,
            $($field_name: $field_type),*
        }

        impl RemoveCollectionItemCommand {
            pub fn new($handle_ident: $handle, path: String, index: usize, $($field_name: $field_type),*) -> Self {
                Self {
                    $handle_ident,
                    path,
                    index,
                    value: None,
                    $($field_name),*
                }
            }
        }

        impl $command for RemoveCollectionItemCommand {
            fn name(&mut $self, _: &$ctx) -> String {
                format!("Remove collection {} item {}", $self.path, $self.index)
            }

            fn execute(&mut $self, $ctx_ident: &mut $ctx) {
                try_modify_property($entity_getter, &$self.path, |field| {
                    field.as_list_mut(&mut |result| {
                        if let Some(list) = result {
                            $self.value = list.reflect_remove($self.index);
                        } else {
                            fyrox::core::log::Log::err(format!("Property {} is not a collection!", $self.path))
                        }
                    })
                })
            }

            fn revert(&mut $self, $ctx_ident: &mut $ctx) {
                try_modify_property($entity_getter, &$self.path, |field| {
                    field.as_list_mut(&mut |result| {
                         if let Some(list) = result {
                            if let Err(item) =
                                list.reflect_insert($self.index, $self.value.take().unwrap())
                            {
                                $self.value = Some(item);
                                fyrox::core::log::Log::err(format!(
                                    "Failed to insert item to {} collection. Type mismatch!",
                                    $self.path
                                ))
                            }
                        } else {
                            fyrox::core::log::Log::err(format!("Property {} is not a collection!", $self.path))
                        }
                    });
                })
            }
        }
    };
}
