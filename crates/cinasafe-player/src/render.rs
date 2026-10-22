//! mpv render API (OpenGL), for drawing video into a GL surface the app owns.
//! Used where `wid` embedding can't put the web UI on top of the video.

use std::ffi::{c_void, CString};
use std::ptr;

use crate::ffi::*;
use crate::{Error, Mpv, Result};

pub type UpdateCallback = Box<dyn Fn() + Send + Sync>;

/// What `mpv_render_context_update` reported.
#[derive(Clone, Copy, Debug)]
pub struct RenderUpdate {
    pub new_frame: bool,
}

pub struct GlRenderContext {
    api: &'static Api,
    ctx: *mut mpv_render_context,
    // Boxed twice so the pointer handed to mpv stays stable.
    callback: Option<Box<UpdateCallback>>,
}

/// Optional native display handles; passing them enables hwdec interop.
pub enum NativeDisplay {
    None,
    X11(*mut c_void),
    Wayland(*mut c_void),
}

impl GlRenderContext {
    /// # Safety
    /// The GL context that will be used for rendering must be current on this
    /// thread, `get_proc_address` must be valid for it, and the returned value
    /// must be dropped (with the context current) before `mpv` is dropped.
    pub unsafe fn new(
        mpv: &Mpv,
        get_proc_address: GetProcAddressFn,
        get_proc_ctx: *mut c_void,
        display: NativeDisplay,
    ) -> Result<GlRenderContext> {
        let api_type = CString::new("opengl").unwrap();
        let mut init =
            mpv_opengl_init_params { get_proc_address: Some(get_proc_address), get_proc_address_ctx: get_proc_ctx };
        let mut params = vec![
            mpv_render_param { type_: MPV_RENDER_PARAM_API_TYPE, data: api_type.as_ptr() as *mut c_void },
            mpv_render_param { type_: MPV_RENDER_PARAM_OPENGL_INIT_PARAMS, data: &mut init as *mut _ as *mut c_void },
        ];
        match display {
            NativeDisplay::None => {}
            NativeDisplay::X11(d) => params.push(mpv_render_param { type_: MPV_RENDER_PARAM_X11_DISPLAY, data: d }),
            NativeDisplay::Wayland(d) => params.push(mpv_render_param { type_: MPV_RENDER_PARAM_WL_DISPLAY, data: d }),
        }
        params.push(mpv_render_param { type_: MPV_RENDER_PARAM_INVALID, data: ptr::null_mut() });

        let api = mpv.api();
        let mut ctx = ptr::null_mut();
        let rc = unsafe { (api.mpv_render_context_create)(&mut ctx, mpv.raw(), params.as_mut_ptr()) };
        mpv.check(|| "render_context_create".into(), rc)?;
        Ok(GlRenderContext { api, ctx, callback: None })
    }

    /// `f` runs on an mpv thread whenever a new frame is ready. It must only
    /// schedule a redraw (e.g. post to the UI loop); it must not call into mpv.
    pub fn set_update_callback(&mut self, f: impl Fn() + Send + Sync + 'static) {
        unsafe extern "C" fn trampoline(data: *mut c_void) {
            let f = unsafe { &*(data as *const UpdateCallback) };
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
        }
        let cb: Box<UpdateCallback> = Box::new(Box::new(f));
        let data = &*cb as *const UpdateCallback as *mut c_void;
        unsafe { (self.api.mpv_render_context_set_update_callback)(self.ctx, Some(trampoline), data) };
        // Replacing the box only after mpv has the new pointer keeps the old one alive until then.
        self.callback = Some(cb);
    }

    pub fn update(&self) -> RenderUpdate {
        let flags = unsafe { (self.api.mpv_render_context_update)(self.ctx) };
        RenderUpdate { new_frame: flags & MPV_RENDER_UPDATE_FRAME != 0 }
    }

    /// Renders the current frame into `fbo` (0 = default framebuffer).
    pub fn render(&self, fbo: i32, width: i32, height: i32, flip_y: bool) -> Result<()> {
        let mut target = mpv_opengl_fbo { fbo, w: width, h: height, internal_format: 0 };
        let mut flip: i32 = flip_y as i32;
        let mut params = [
            mpv_render_param { type_: MPV_RENDER_PARAM_OPENGL_FBO, data: &mut target as *mut _ as *mut c_void },
            mpv_render_param { type_: MPV_RENDER_PARAM_FLIP_Y, data: &mut flip as *mut _ as *mut c_void },
            mpv_render_param { type_: MPV_RENDER_PARAM_INVALID, data: ptr::null_mut() },
        ];
        let rc = unsafe { (self.api.mpv_render_context_render)(self.ctx, params.as_mut_ptr()) };
        if rc < 0 {
            return Err(Error::Mpv { what: "render".into(), code: rc, msg: String::new() });
        }
        Ok(())
    }

    pub fn report_swap(&self) {
        unsafe { (self.api.mpv_render_context_report_swap)(self.ctx) }
    }
}

impl Drop for GlRenderContext {
    fn drop(&mut self) {
        unsafe {
            (self.api.mpv_render_context_set_update_callback)(self.ctx, None, ptr::null_mut());
            (self.api.mpv_render_context_free)(self.ctx);
        }
    }
}
