import org.mozilla.javascript.WrapFactory;

/**
 * How a Java value reaches a script. A Java string, number or boolean arrives as the JavaScript
 * primitive (measured: {@code typeof} a list element is {@code "string"}, and a boxed Integer is
 * a plain number). A list a binding hands out is a {@link SequenceFacade}, which the engine
 * presents as {@code [object JavaList]}.
 */
final class BindingValuePresenter extends WrapFactory {
  BindingValuePresenter() {
    setJavaPrimitiveWrap(false);
  }
}
