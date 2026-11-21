// Select vkd3d-proton's storage-buffer CBV path on hardware whose uniform
// buffers cannot preserve the 16-byte offsets used by Frosty's descriptor tables.
#include <vulkan/vk_layer.h>
#include <pthread.h>
#include <stdlib.h>
#include <string.h>

struct instance_dispatch {
    void *key;
    PFN_vkGetInstanceProcAddr get_proc;
    PFN_vkDestroyInstance destroy;
    PFN_vkGetPhysicalDeviceProperties properties;
    PFN_vkGetPhysicalDeviceFeatures2 core_features;
    PFN_vkGetPhysicalDeviceFeatures2KHR khr_features;
    struct instance_dispatch *next;
};

static struct instance_dispatch *instances;
static pthread_mutex_t instances_lock = PTHREAD_MUTEX_INITIALIZER;

// The loader assigns the same dispatch-table pointer to an instance and its
// physical devices. Vulkan requires callers to synchronize instance destruction.
static void *dispatch_key(const void *object) {
    void *key;
    memcpy(&key, object, sizeof(key));
    return key;
}

static struct instance_dispatch *lookup(const void *object) {
    void *key = dispatch_key(object);
    pthread_mutex_lock(&instances_lock);
    struct instance_dispatch *entry = instances;
    while (entry && entry->key != key)
        entry = entry->next;
    pthread_mutex_unlock(&instances_lock);
    return entry;
}

static VKAPI_ATTR VkResult VKAPI_CALL create_instance(const VkInstanceCreateInfo *info,
        const VkAllocationCallbacks *allocator, VkInstance *instance) {
    VkLayerInstanceCreateInfo *chain = (VkLayerInstanceCreateInfo *)info->pNext;
    while (chain && (chain->sType != VK_STRUCTURE_TYPE_LOADER_INSTANCE_CREATE_INFO ||
                     chain->function != VK_LAYER_LINK_INFO))
        chain = (VkLayerInstanceCreateInfo *)chain->pNext;
    if (!chain || !chain->u.pLayerInfo)
        return VK_ERROR_INITIALIZATION_FAILED;

    PFN_vkGetInstanceProcAddr next = chain->u.pLayerInfo->pfnNextGetInstanceProcAddr;
    PFN_vkCreateInstance create = (PFN_vkCreateInstance)next(NULL, "vkCreateInstance");
    if (!create)
        return VK_ERROR_INITIALIZATION_FAILED;
    struct instance_dispatch *entry = malloc(sizeof(*entry));
    if (!entry)
        return VK_ERROR_OUT_OF_HOST_MEMORY;
    chain->u.pLayerInfo = chain->u.pLayerInfo->pNext;
    VkResult result = create(info, allocator, instance);
    if (result != VK_SUCCESS) {
        free(entry);
        return result;
    }

    entry->key = dispatch_key(*instance);
    entry->get_proc = next;
    entry->destroy = (PFN_vkDestroyInstance)next(*instance, "vkDestroyInstance");
    entry->properties = (PFN_vkGetPhysicalDeviceProperties)next(*instance, "vkGetPhysicalDeviceProperties");
    entry->core_features = (PFN_vkGetPhysicalDeviceFeatures2)next(*instance, "vkGetPhysicalDeviceFeatures2");
    entry->khr_features = (PFN_vkGetPhysicalDeviceFeatures2KHR)next(*instance, "vkGetPhysicalDeviceFeatures2KHR");
    pthread_mutex_lock(&instances_lock);
    entry->next = instances;
    instances = entry;
    pthread_mutex_unlock(&instances_lock);
    return result;
}

static VKAPI_ATTR void VKAPI_CALL destroy_instance(VkInstance instance, const VkAllocationCallbacks *allocator) {
    if (!instance)
        return;
    void *key = dispatch_key(instance);
    pthread_mutex_lock(&instances_lock);
    struct instance_dispatch **link = &instances;
    while (*link && (*link)->key != key)
        link = &(*link)->next;
    struct instance_dispatch *entry = *link;
    if (entry)
        *link = entry->next;
    pthread_mutex_unlock(&instances_lock);
    if (entry) {
        entry->destroy(instance, allocator);
        free(entry);
    }
}

static void select_cbv_type(const struct instance_dispatch *entry, VkPhysicalDevice device,
        VkPhysicalDeviceFeatures2 *features) {
    VkPhysicalDeviceProperties props;
    entry->properties(device, &props);
    if (props.limits.minUniformBufferOffsetAlignment <= 16 || props.limits.minStorageBufferOffsetAlignment > 16)
        return;

    for (VkBaseOutStructure *node = features->pNext; node; node = node->pNext) {
        if (node->sType == VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_VULKAN_1_2_FEATURES) {
            VkPhysicalDeviceVulkan12Features *v12 = (void *)node;
            if (v12->descriptorBindingStorageBufferUpdateAfterBind && v12->shaderStorageBufferArrayNonUniformIndexing)
                v12->descriptorBindingUniformBufferUpdateAfterBind = VK_FALSE;
        } else if (node->sType == VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_DESCRIPTOR_INDEXING_FEATURES) {
            VkPhysicalDeviceDescriptorIndexingFeatures *indexing = (void *)node;
            if (indexing->descriptorBindingStorageBufferUpdateAfterBind && indexing->shaderStorageBufferArrayNonUniformIndexing)
                indexing->descriptorBindingUniformBufferUpdateAfterBind = VK_FALSE;
        }
    }
}

static VKAPI_ATTR void VKAPI_CALL get_core_features(VkPhysicalDevice device, VkPhysicalDeviceFeatures2 *features) {
    const struct instance_dispatch *entry = lookup(device);
    entry->core_features(device, features);
    select_cbv_type(entry, device, features);
}

static VKAPI_ATTR void VKAPI_CALL get_khr_features(VkPhysicalDevice device, VkPhysicalDeviceFeatures2 *features) {
    const struct instance_dispatch *entry = lookup(device);
    entry->khr_features(device, features);
    select_cbv_type(entry, device, features);
}

static VKAPI_ATTR PFN_vkVoidFunction VKAPI_CALL get_proc(VkInstance instance, const char *name) {
    if (!strcmp(name, "vkCreateInstance"))
        return (PFN_vkVoidFunction)create_instance;
    if (!strcmp(name, "vkGetInstanceProcAddr"))
        return (PFN_vkVoidFunction)get_proc;
    if (!instance)
        return NULL;
    const struct instance_dispatch *entry = lookup(instance);
    if (!entry)
        return NULL;
    if (!strcmp(name, "vkDestroyInstance"))
        return (PFN_vkVoidFunction)destroy_instance;
    if (!strcmp(name, "vkGetPhysicalDeviceFeatures2"))
        return entry->core_features ? (PFN_vkVoidFunction)get_core_features : NULL;
    if (!strcmp(name, "vkGetPhysicalDeviceFeatures2KHR"))
        return entry->khr_features ? (PFN_vkVoidFunction)get_khr_features : NULL;
    return entry->get_proc(instance, name);
}

VKAPI_ATTR VkResult VKAPI_CALL vkNegotiateLoaderLayerInterfaceVersion(VkNegotiateLayerInterface *info) {
    if (!info || info->sType != LAYER_NEGOTIATE_INTERFACE_STRUCT || info->loaderLayerInterfaceVersion < 2)
        return VK_ERROR_INITIALIZATION_FAILED;
    info->loaderLayerInterfaceVersion = 2;
    info->pfnGetInstanceProcAddr = get_proc;
    // The device dispatch chain and physical-device extension commands pass through.
    info->pfnGetDeviceProcAddr = NULL;
    info->pfnGetPhysicalDeviceProcAddr = NULL;
    return VK_SUCCESS;
}
