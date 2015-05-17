/// The Trace trait which needs to be implemented on garbage collected objects
trait Trace {

    /// Mark all contained Gcs
    fn trace(&self);

    // todo: these should be unsafe, need compiler support for the plugin
    /// Increment the root-count of all contained Gcs
    fn root(&self);

    /// Decrement the root-count of all contained Gcs
    fn unroot(&self);
}


#[test]
fn it_works() {
}
