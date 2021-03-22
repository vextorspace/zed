use crate::{
    app::{AppContext, MutableAppContext, WindowInvalidation},
    elements::Element,
    fonts::FontCache,
    platform::Event,
    text_layout::TextLayoutCache,
    AssetCache, ElementBox, Scene,
};
use pathfinder_geometry::vector::{vec2f, Vector2F};
use std::{any::Any, collections::HashMap, sync::Arc};

pub struct Presenter {
    window_id: usize,
    rendered_views: HashMap<usize, ElementBox>,
    parents: HashMap<usize, usize>,
    font_cache: Arc<FontCache>,
    text_layout_cache: TextLayoutCache,
    asset_cache: Arc<AssetCache>,
}

impl Presenter {
    pub fn new(
        window_id: usize,
        font_cache: Arc<FontCache>,
        asset_cache: Arc<AssetCache>,
        app: &MutableAppContext,
    ) -> Self {
        Self {
            window_id,
            rendered_views: app.render_views(window_id).unwrap(),
            parents: HashMap::new(),
            font_cache,
            text_layout_cache: TextLayoutCache::new(),
            asset_cache,
        }
    }

    pub fn dispatch_path(&self, app: &AppContext) -> Vec<usize> {
        let mut view_id = app.focused_view_id(self.window_id).unwrap();
        let mut path = vec![view_id];
        while let Some(parent_id) = self.parents.get(&view_id).copied() {
            path.push(parent_id);
            view_id = parent_id;
        }
        path.reverse();
        path
    }

    pub fn invalidate(&mut self, invalidation: WindowInvalidation, app: &AppContext) {
        for view_id in invalidation.updated {
            self.rendered_views
                .insert(view_id, app.render_view(self.window_id, view_id).unwrap());
        }
        for view_id in invalidation.removed {
            self.rendered_views.remove(&view_id);
            self.parents.remove(&view_id);
        }
    }

    pub fn build_scene(
        &mut self,
        window_size: Vector2F,
        scale_factor: f32,
        app: &mut MutableAppContext,
    ) -> Scene {
        let mut scene = Scene::new(scale_factor);

        if let Some(root_view_id) = app.root_view_id(self.window_id) {
            self.layout(window_size, app.downgrade());
            self.after_layout(app);
            let mut ctx = PaintContext {
                scene: &mut scene,
                font_cache: &self.font_cache,
                text_layout_cache: &self.text_layout_cache,
                rendered_views: &mut self.rendered_views,
                app: app.downgrade(),
            };
            ctx.paint(root_view_id, Vector2F::zero());
            self.text_layout_cache.finish_frame();
        } else {
            log::error!("could not find root_view_id for window {}", self.window_id);
        }

        scene
    }

    fn layout(&mut self, size: Vector2F, app: &AppContext) {
        if let Some(root_view_id) = app.root_view_id(self.window_id) {
            let mut layout_ctx = LayoutContext {
                rendered_views: &mut self.rendered_views,
                parents: &mut self.parents,
                font_cache: &self.font_cache,
                text_layout_cache: &self.text_layout_cache,
                asset_cache: &self.asset_cache,
                view_stack: Vec::new(),
                app,
            };
            layout_ctx.layout(root_view_id, SizeConstraint::strict(size));
        }
    }

    fn after_layout(&mut self, app: &mut MutableAppContext) {
        if let Some(root_view_id) = app.root_view_id(self.window_id) {
            let mut ctx = AfterLayoutContext {
                rendered_views: &mut self.rendered_views,
                font_cache: &self.font_cache,
                text_layout_cache: &self.text_layout_cache,
                app,
            };
            ctx.after_layout(root_view_id);
        }
    }

    pub fn dispatch_event(&mut self, event: Event, app: &AppContext) -> Vec<ActionToDispatch> {
        if let Some(root_view_id) = app.root_view_id(self.window_id) {
            let mut ctx = EventContext {
                rendered_views: &mut self.rendered_views,
                actions: Vec::new(),
                font_cache: &self.font_cache,
                text_layout_cache: &self.text_layout_cache,
                view_stack: Vec::new(),
                app,
            };
            ctx.dispatch_event(root_view_id, &event);
            ctx.actions
        } else {
            Vec::new()
        }
    }
}

pub struct ActionToDispatch {
    pub path: Vec<usize>,
    pub name: &'static str,
    pub arg: Box<dyn Any>,
}

pub struct LayoutContext<'a> {
    rendered_views: &'a mut HashMap<usize, ElementBox>,
    parents: &'a mut HashMap<usize, usize>,
    pub font_cache: &'a FontCache,
    pub text_layout_cache: &'a TextLayoutCache,
    pub asset_cache: &'a AssetCache,
    pub app: &'a AppContext,
    view_stack: Vec<usize>,
}

impl<'a> LayoutContext<'a> {
    fn layout(&mut self, view_id: usize, constraint: SizeConstraint) -> Vector2F {
        if let Some(parent_id) = self.view_stack.last() {
            self.parents.insert(view_id, *parent_id);
        }
        self.view_stack.push(view_id);
        let mut rendered_view = self.rendered_views.remove(&view_id).unwrap();
        let size = rendered_view.layout(constraint, self);
        self.rendered_views.insert(view_id, rendered_view);
        self.view_stack.pop();
        size
    }
}

pub struct AfterLayoutContext<'a> {
    rendered_views: &'a mut HashMap<usize, ElementBox>,
    pub font_cache: &'a FontCache,
    pub text_layout_cache: &'a TextLayoutCache,
    pub app: &'a mut MutableAppContext,
}

impl<'a> AfterLayoutContext<'a> {
    fn after_layout(&mut self, view_id: usize) {
        if let Some(mut view) = self.rendered_views.remove(&view_id) {
            view.after_layout(self);
            self.rendered_views.insert(view_id, view);
        }
    }
}

pub struct PaintContext<'a> {
    rendered_views: &'a mut HashMap<usize, ElementBox>,
    pub scene: &'a mut Scene,
    pub font_cache: &'a FontCache,
    pub text_layout_cache: &'a TextLayoutCache,
    pub app: &'a AppContext,
}

impl<'a> PaintContext<'a> {
    fn paint(&mut self, view_id: usize, origin: Vector2F) {
        if let Some(mut tree) = self.rendered_views.remove(&view_id) {
            tree.paint(origin, self);
            self.rendered_views.insert(view_id, tree);
        }
    }
}

pub struct EventContext<'a> {
    rendered_views: &'a mut HashMap<usize, ElementBox>,
    actions: Vec<ActionToDispatch>,
    pub font_cache: &'a FontCache,
    pub text_layout_cache: &'a TextLayoutCache,
    pub app: &'a AppContext,
    view_stack: Vec<usize>,
}

impl<'a> EventContext<'a> {
    fn dispatch_event(&mut self, view_id: usize, event: &Event) -> bool {
        if let Some(mut element) = self.rendered_views.remove(&view_id) {
            self.view_stack.push(view_id);
            let result = element.dispatch_event(event, self);
            self.view_stack.pop();
            self.rendered_views.insert(view_id, element);
            result
        } else {
            false
        }
    }

    pub fn dispatch_action<A: 'static + Any>(&mut self, name: &'static str, arg: A) {
        self.actions.push(ActionToDispatch {
            path: self.view_stack.clone(),
            name,
            arg: Box::new(arg),
        });
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Axis {
    Horizontal,
    Vertical,
}

impl Axis {
    pub fn invert(self) -> Self {
        match self {
            Self::Horizontal => Self::Vertical,
            Self::Vertical => Self::Horizontal,
        }
    }
}

pub trait Vector2FExt {
    fn along(self, axis: Axis) -> f32;
}

impl Vector2FExt for Vector2F {
    fn along(self, axis: Axis) -> f32 {
        match axis {
            Axis::Horizontal => self.x(),
            Axis::Vertical => self.y(),
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub struct SizeConstraint {
    pub min: Vector2F,
    pub max: Vector2F,
}

impl SizeConstraint {
    pub fn new(min: Vector2F, max: Vector2F) -> Self {
        Self { min, max }
    }

    pub fn strict(size: Vector2F) -> Self {
        Self {
            min: size,
            max: size,
        }
    }

    pub fn strict_along(axis: Axis, max: f32) -> Self {
        match axis {
            Axis::Horizontal => Self {
                min: vec2f(max, 0.0),
                max: vec2f(max, f32::INFINITY),
            },
            Axis::Vertical => Self {
                min: vec2f(0.0, max),
                max: vec2f(f32::INFINITY, max),
            },
        }
    }

    pub fn max_along(&self, axis: Axis) -> f32 {
        match axis {
            Axis::Horizontal => self.max.x(),
            Axis::Vertical => self.max.y(),
        }
    }
}

pub struct ChildView {
    view_id: usize,
}

impl ChildView {
    pub fn new(view_id: usize) -> Self {
        Self { view_id }
    }
}

impl Element for ChildView {
    type LayoutState = ();
    type PaintState = ();

    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
    ) -> (Vector2F, Self::LayoutState) {
        let size = ctx.layout(self.view_id, constraint);
        (size, ())
    }

    fn after_layout(
        &mut self,
        _: Vector2F,
        _: &mut Self::LayoutState,
        ctx: &mut AfterLayoutContext,
    ) {
        ctx.after_layout(self.view_id);
    }

    fn paint(
        &mut self,
        bounds: pathfinder_geometry::rect::RectF,
        _: &mut Self::LayoutState,
        ctx: &mut PaintContext,
    ) -> Self::PaintState {
        ctx.paint(self.view_id, bounds.origin());
    }

    fn dispatch_event(
        &mut self,
        event: &Event,
        _: pathfinder_geometry::rect::RectF,
        _: &mut Self::LayoutState,
        _: &mut Self::PaintState,
        ctx: &mut EventContext,
    ) -> bool {
        ctx.dispatch_event(self.view_id, event)
    }
}

#[cfg(test)]
mod tests {
    // #[test]
    // fn test_responder_chain() {
    //     let settings = settings_rx(None);
    //     let mut app = App::new().unwrap();
    //     let workspace = app.add_model(|ctx| Workspace::new(Vec::new(), ctx));
    //     let (window_id, workspace_view) =
    //         app.add_window(|ctx| WorkspaceView::new(workspace.clone(), settings, ctx));

    //     let invalidations = Rc::new(RefCell::new(Vec::new()));
    //     let invalidations_ = invalidations.clone();
    //     app.on_window_invalidated(window_id, move |invalidation, _| {
    //         invalidations_.borrow_mut().push(invalidation)
    //     });

    //     let active_pane_id = workspace_view.update(&mut app, |view, ctx| {
    //         ctx.focus(view.active_pane());
    //         view.active_pane().id()
    //     });

    //     app.update(|app| {
    //         let mut presenter = Presenter::new(
    //             window_id,
    //             Rc::new(FontCache::new()),
    //             Rc::new(AssetCache::new()),
    //             app,
    //         );
    //         for invalidation in invalidations.borrow().iter().cloned() {
    //             presenter.update(vec2f(1024.0, 768.0), 2.0, Some(invalidation), app);
    //         }

    //         assert_eq!(
    //             presenter.responder_chain(app.ctx()).unwrap(),
    //             vec![workspace_view.id(), active_pane_id]
    //         );
    //     });
    // }
}
