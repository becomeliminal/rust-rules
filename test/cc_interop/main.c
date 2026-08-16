#include <stdio.h>
int ffi_add(int a, int b);
int main(void) {
    printf("rust says %d\n", ffi_add(40, 2));
    return 0;
}
