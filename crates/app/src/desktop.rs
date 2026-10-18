//! OS document requests are queued until the editor can accept them.
use std::{path::PathBuf,sync::Mutex};
static PATHS:Mutex<Vec<PathBuf>>=Mutex::new(Vec::new());
static CONTEXT:Mutex<Option<eframe::egui::Context>>=Mutex::new(None);
pub fn enqueue(path:PathBuf) {
    let mut queue=PATHS.lock().unwrap();
    if !queue.contains(&path) { queue.push(path); }
    drop(queue);
    if let Some(ctx)=CONTEXT.lock().unwrap().as_ref() {ctx.request_repaint();}
}
pub fn attach(ctx:&eframe::egui::Context) {*CONTEXT.lock().unwrap()=Some(ctx.clone());}
pub fn take()->Vec<PathBuf> {std::mem::take(&mut *PATHS.lock().unwrap())}
pub fn project_path(path:&std::path::Path)->bool {
    path.extension().and_then(|s|s.to_str()).is_some_and(|s|s.eq_ignore_ascii_case("genix")||s.eq_ignore_ascii_case("ostov"))
}
#[cfg(target_os="macos")]
mod mac {
    use super::*;
    use objc2::{declare_class,msg_send_id,msg_send,sel,mutability,ClassType,DeclaredClass};
    use objc2::rc::Retained;
    use objc2_foundation::{NSAppleEventManager,NSAppleEventDescriptor,NSNotificationCenter,NSString,MainThreadMarker,NSObject,NSObjectProtocol};
    declare_class!(
        struct GenixDelegate;
        unsafe impl ClassType for GenixDelegate {
            type Super=NSObject;
            type Mutability=mutability::MainThreadOnly;
            const NAME:&'static str="GenixDocumentEventHandler";
        }
        impl DeclaredClass for GenixDelegate {}
        unsafe impl NSObjectProtocol for GenixDelegate {}
        unsafe impl GenixDelegate {
            #[method(willFinishLaunching:)]
            fn will_finish_launching(&self,_notification:&NSObject) {register_handler(self);}
            #[method(handleOpenDocuments:withReplyEvent:)]
            fn open_documents(&self,event:&NSAppleEventDescriptor,_reply:Option<&NSAppleEventDescriptor>) {
                unsafe {
                    let list:Option<Retained<NSAppleEventDescriptor>>=msg_send_id![event,paramDescriptorForKeyword:u32::from_be_bytes(*b"----")];
                    if let Some(list)=list {
                        for index in 1..=list.numberOfItems() {
                            if let Some(item)=list.descriptorAtIndex(index) {
                                let url_item:Option<Retained<NSAppleEventDescriptor>>=msg_send_id![&*item,coerceToDescriptorType:u32::from_be_bytes(*b"furl")];
                                let url_item=url_item.as_deref().unwrap_or(&item);
                                if let Some(url)=url_item.fileURLValue() {if let Some(path)=url.path() {enqueue(PathBuf::from(path.to_string()));}}
                            }
                        }
                    }
                }
            }
        }
    );
    fn register_handler(delegate:&GenixDelegate) {
        unsafe {
            let manager=NSAppleEventManager::sharedAppleEventManager();
            let _:()=msg_send![&*manager,setEventHandler:delegate andSelector:sel!(handleOpenDocuments:withReplyEvent:) forEventClass:u32::from_be_bytes(*b"aevt") andEventID:u32::from_be_bytes(*b"odoc")];
        }
    }
    thread_local! {static DELEGATE:std::cell::RefCell<Option<Retained<GenixDelegate>>>=const {std::cell::RefCell::new(None)};}
    pub fn install() {
        let mtm=MainThreadMarker::new().expect("AppKit main thread");
        DELEGATE.with(|slot| {
            if slot.borrow().is_none() {
                let delegate:Retained<GenixDelegate>=unsafe {msg_send_id![super(mtm.alloc().set_ivars(())),init]};
                unsafe {
                    let center=NSNotificationCenter::defaultCenter();
                    let name=NSString::from_str("NSApplicationWillFinishLaunchingNotification");
                    let object:Option<&NSObject>=None;
                    let _:()=msg_send![&*center,addObserver:&*delegate selector:sel!(willFinishLaunching:) name:&*name object:object];
                }
                *slot.borrow_mut()=Some(delegate);
            }
            let borrowed=slot.borrow();let delegate=borrowed.as_ref().unwrap();
            // AppKit installs its handlers during launch. Register again before
            // initial document dispatch, without replacing Winit's lifecycle delegate.
            register_handler(delegate);
        });
    }
}
#[cfg(target_os="macos")]
pub use mac::install;
#[cfg(not(target_os="macos"))]
pub fn install() {}
#[cfg(target_os="macos")]
pub fn reveal(path:&std::path::Path) {let _=std::process::Command::new("/usr/bin/open").arg("-R").arg(path).spawn();}
#[cfg(not(target_os="macos"))]
pub fn reveal(_path:&std::path::Path) {}
#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn project_extensions_are_case_insensitive_and_do_not_accept_unrelated_files() {
        assert!(project_path(std::path::Path::new("Project.GENIX")));
        assert!(project_path(std::path::Path::new("old.ostov")));
        assert!(!project_path(std::path::Path::new("photo.png")));
    }
}
