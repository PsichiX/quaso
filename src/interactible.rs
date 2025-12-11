use spitfire_core::Triangle;
use spitfire_draw::{
    context::DrawContext,
    utils::{Drawable, ShaderRef, Vertex},
};
use spitfire_glow::{
    graphics::{GraphicsBatch, GraphicsTarget},
    renderer::{GlowBlending, GlowUniformValue},
};
use vek::{Aabr, Rect, Rgba, Vec2};

#[derive(Debug, Default, Clone)]
pub struct Interactible {
    pub vertices: Vec<Vec2<f32>>,
    pub triangles: Vec<[u32; 3]>,
}

impl Interactible {
    pub fn new(
        vertices: impl IntoIterator<Item = Vec2<f32>>,
        triangles: impl IntoIterator<Item = [u32; 3]>,
    ) -> Self {
        Self {
            vertices: vertices.into_iter().collect(),
            triangles: triangles.into_iter().collect(),
        }
    }

    pub fn from_rect(rect: Rect<f32, f32>) -> Self {
        let vertices = vec![
            Vec2::new(rect.x, rect.y),
            Vec2::new(rect.x + rect.w, rect.y),
            Vec2::new(rect.x + rect.w, rect.y + rect.h),
            Vec2::new(rect.x, rect.y + rect.h),
        ];
        let triangles = vec![[0, 1, 2], [0, 2, 3]];
        Self {
            vertices,
            triangles,
        }
    }

    pub fn vertex(mut self, vertex: impl Into<Vec2<f32>>) -> Self {
        self.add_vertex(vertex);
        self
    }

    pub fn triangle(mut self, triangle: [u32; 3]) -> Self {
        self.add_triangle(triangle);
        self
    }

    pub fn add_vertex(&mut self, vertex: impl Into<Vec2<f32>>) {
        self.vertices.push(vertex.into());
    }

    pub fn add_triangle(&mut self, triangle: [u32; 3]) {
        self.triangles.push(triangle);
    }

    pub fn bounding_box(&self) -> Aabr<f32> {
        if self.vertices.is_empty() {
            return Default::default();
        }

        let mut bbox = Aabr::new_empty(self.vertices[0]);
        for vertex in &self.vertices[1..] {
            bbox = bbox.expanded_to_contain_point(*vertex);
        }
        bbox
    }

    pub fn contains_point(&self, point: impl Into<Vec2<f32>>) -> bool {
        let point = point.into();

        for triangle in &self.triangles {
            let v0 = self.vertices[triangle[0] as usize];
            let v1 = self.vertices[triangle[1] as usize];
            let v2 = self.vertices[triangle[2] as usize];

            let dx = point.x - v2.x;
            let dy = point.y - v2.y;
            let dx21 = v2.x - v1.x;
            let dy12 = v1.y - v2.y;
            let d = dy12 * (v0.x - v2.x) + dx21 * (v0.y - v2.y);
            let s = dy12 * dx + dx21 * dy;
            let t = (v2.y - v0.y) * dx + (v0.x - v2.x) * dy;

            if d < 0.0 {
                if s <= 0.0 && t <= 0.0 && s + t >= d {
                    return true;
                }
            } else if s >= 0.0 && t >= 0.0 && s + t <= d {
                return true;
            }
        }

        false
    }

    pub fn draw_wireframe(
        &self,
        shader: &ShaderRef,
        color: Rgba<f32>,
        context: &mut DrawContext,
        graphics: &mut dyn GraphicsTarget<Vertex>,
        time: f32,
    ) {
        let batch = GraphicsBatch {
            shader: context.shader(Some(shader)),
            uniforms: [
                (
                    "u_projection_view".into(),
                    GlowUniformValue::M4(
                        graphics.state().main_camera.world_matrix().into_col_array(),
                    ),
                ),
                ("u_time".into(), GlowUniformValue::F1(time)),
            ]
            .into(),
            textures: Default::default(),
            blending: GlowBlending::None,
            scissor: None,
            wireframe: true,
        };
        let stream = &mut graphics.state_mut().stream;
        stream.batch_optimized(batch);
        unsafe {
            stream.extend_triangles(
                true,
                self.triangles.iter().map(|v| Triangle {
                    a: v[0],
                    b: v[1],
                    c: v[2],
                }),
            );
            stream.extend_vertices(self.vertices.iter().copied().map(|v| Vertex {
                position: v.into_array(),
                uv: [0.0, 0.0, 0.0],
                color: color.into_array(),
            }));
        }
    }
}

pub struct RenderableInteractible {
    pub interactible: Interactible,
    vertex_offset: usize,
    triangle_offset: usize,
}

impl RenderableInteractible {
    pub fn new(graphics: &mut dyn GraphicsTarget<Vertex>) -> Self {
        Self {
            interactible: Interactible::default(),
            vertex_offset: graphics.state().stream.vertices().len(),
            triangle_offset: graphics.state().stream.triangles().len(),
        }
    }

    pub fn interactible(mut self, graphics: &mut dyn GraphicsTarget<Vertex>) -> Interactible {
        self.interactible
            .vertices
            .reserve_exact(graphics.state().stream.vertices().len() - self.vertex_offset);
        self.interactible
            .triangles
            .reserve_exact(graphics.state().stream.triangles().len() - self.triangle_offset);
        for vertex in graphics.state().stream.vertices()[self.vertex_offset..].iter() {
            self.interactible.add_vertex(vertex.position);
        }
        for triangle in graphics.state().stream.triangles()[self.triangle_offset..].iter() {
            self.interactible.add_triangle([
                triangle.a - self.vertex_offset as u32,
                triangle.b - self.vertex_offset as u32,
                triangle.c - self.vertex_offset as u32,
            ]);
        }
        self.interactible
    }

    pub fn vertex_offset(&self) -> usize {
        self.vertex_offset
    }

    pub fn triangle_offset(&self) -> usize {
        self.triangle_offset
    }
}

pub struct DrawableInteractible<T: Drawable> {
    pub drawable: T,
}

impl<T: Drawable> DrawableInteractible<T> {
    pub fn new(drawable: T) -> Self {
        Self { drawable }
    }

    pub fn draw(
        self,
        context: &mut DrawContext,
        graphics: &mut dyn GraphicsTarget<Vertex>,
    ) -> Interactible {
        let interactible = RenderableInteractible::new(graphics);
        self.drawable.draw(context, graphics);
        interactible.interactible(graphics)
    }
}
